use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WATCH_RELOAD_JS: &str = "<script data-watch-reload>(function(){var v;setInterval(function(){fetch(\"/_reload\",{cache:\"no-store\"}).then(function(r){return r.ok?r.text():v}).then(function(t){if(v!==undefined)if(t!==v)location.reload();v=t}).catch(function(){})},1000)})()</script>";

#[derive(Clone)]
struct ServerConfig {
    port: u16,
    serve_dir: PathBuf,
    watch: bool,
    open_browser: bool,
    builder_bin: PathBuf,
    wiki_name: String,
    wiki_dir: PathBuf,
    source_dir: PathBuf,
    template_dir: PathBuf,
    static_dir: PathBuf,
    assets_dir: PathBuf,
    cache_dir: PathBuf,
    stage_dir: PathBuf,
    config_tsv: PathBuf,
    domain: String,
    jobs: usize,
}

struct AppState {
    cfg: ServerConfig,
    reload_token: AtomicU64,
    boot_epoch: u64,
}

fn main() {
    let cfg = parse_cli();

    let boot_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let state = Arc::new(AppState {
        cfg: cfg.clone(),
        reload_token: AtomicU64::new(0),
        boot_epoch,
    });

    if cfg.watch {
        let _ = fs::create_dir_all(&cfg.serve_dir);
        let _ = fs::create_dir_all(&cfg.stage_dir);
        let _ = fs::create_dir_all(&cfg.cache_dir);

        // Initial build before accepting connections
        let mut last_shared = load_shared_fingerprint(&cfg.cache_dir);
        if run_watch_cycle(&state, &mut last_shared).is_err() {
            eprintln!("wiki:error: Initial watch build failed.");
            std::process::exit(1);
        }

        // Spawn background file watcher thread
        let watcher_state = Arc::clone(&state);
        thread::spawn(move || watch_loop(watcher_state, last_shared));
    } else if !cfg.serve_dir.is_dir() {
        eprintln!(
            "wiki:error: Serve directory '{}' not found.",
            cfg.serve_dir.display()
        );
        std::process::exit(1);
    }

    let bind_addr = format!("0.0.0.0:{}", cfg.port);
    let listener = match TcpListener::bind(&bind_addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "wiki:error: Could not bind to port {} ({}). Is another server running?",
                cfg.port, e
            );
            std::process::exit(1);
        }
    };

    let url = format!("http://127.0.0.1:{}", cfg.port);
    if cfg.watch {
        println!(
            "wiki:info: Watching content/ & templates/ and serving '{}' on {} (Press Ctrl+C to stop)",
            cfg.wiki_name, url
        );
    } else {
        println!(
            "wiki:info: Serving '{}' on {} (Press Ctrl+C to stop)",
            cfg.serve_dir.display(),
            url
        );
    }

    if cfg.open_browser {
        let open_url = url.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            open_in_browser(&open_url);
        });
    }

    for stream in listener.incoming().flatten() {
        let st = Arc::clone(&state);
        thread::spawn(move || {
            let _ = handle_client(stream, &st);
        });
    }
}

fn handle_client(mut stream: TcpStream, state: &AppState) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    if n == 0 {
        return Ok(());
    }

    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let raw_target = parts.next().unwrap_or("/");

    if method != "GET" && method != "HEAD" {
        return write_response(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"Method Not Allowed",
            method == "HEAD",
        );
    }

    let path_only = raw_target.split(['?', '#']).next().unwrap_or("/");
    let decoded = percent_decode(path_only);

    // Live-reload polling endpoint
    if state.cfg.watch && decoded == "/_reload" {
        let ver = state.reload_token.load(Ordering::Relaxed);
        let body = format!("{}.{}\n", state.boot_epoch, ver);
        return write_response_no_cache(
            &mut stream,
            "200 OK",
            "text/plain; charset=utf-8",
            body.as_bytes(),
            method == "HEAD",
        );
    }

    let Some(file_path) = resolve_safe_path(&state.cfg.serve_dir, &decoded) else {
        return write_response(
            &mut stream,
            "404 Not Found",
            "text/html; charset=utf-8",
            b"<h1>404 Not Found</h1>",
            method == "HEAD",
        );
    };

    let Ok(mut bytes) = fs::read(&file_path) else {
        return write_response(
            &mut stream,
            "404 Not Found",
            "text/html; charset=utf-8",
            b"<h1>404 Not Found</h1>",
            method == "HEAD",
        );
    };

    let mime = guess_mime(&file_path);

    // Inject live-reload script on-the-fly for HTML responses in watch mode
    if state.cfg.watch && mime.starts_with("text/html") && !contains_subslice(&bytes, b"data-watch-reload") {
        if let Some(pos) = rfind_subslice(&bytes, b"</body>") {
            let mut injected = Vec::with_capacity(bytes.len() + WATCH_RELOAD_JS.len());
            injected.extend_from_slice(&bytes[..pos]);
            injected.extend_from_slice(WATCH_RELOAD_JS.as_bytes());
            injected.extend_from_slice(&bytes[pos..]);
            bytes = injected;
        } else {
            bytes.extend_from_slice(WATCH_RELOAD_JS.as_bytes());
        }
    }

    if state.cfg.watch {
        write_response_no_cache(&mut stream, "200 OK", mime, &bytes, method == "HEAD")
    } else {
        write_response(&mut stream, "200 OK", mime, &bytes, method == "HEAD")
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        content_type,
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    if !head_only {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn write_response_no_cache(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store, no-cache, must-revalidate\r\nConnection: close\r\n\r\n",
        status,
        content_type,
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    if !head_only {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn resolve_safe_path(base: &Path, req_path: &str) -> Option<PathBuf> {
    let trimmed = req_path.trim_start_matches('/');
    let mut clean = PathBuf::from(base);
    for comp in Path::new(trimmed).components() {
        match comp {
            Component::Normal(seg) => clean.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if clean.is_dir() {
        let idx = clean.join("index.html");
        if idx.is_file() {
            return Some(idx);
        }
    }
    if clean.is_file() {
        return Some(clean);
    }
    if clean.extension().is_none() {
        let with_html = clean.with_extension("html");
        if with_html.is_file() {
            return Some(with_html);
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn guess_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" | "webmanifest" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "xml" => "application/xml; charset=utf-8",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "pdf" => "application/pdf",
        "txt" | "md" | "wiki" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn rfind_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .rposition(|w| w == needle)
}

// ============================================================================
// Watch Loop & Atomic Staging Sync (Native replacement for Makefile WATCH=1)
// ============================================================================

fn watch_loop(state: Arc<AppState>, mut last_shared: u64) {
    let watch_roots = collect_watch_roots(&state.cfg);
    let mut last_stat_map = stat_snapshot(&watch_roots);
    let mut last_hash_map = content_snapshot(&watch_roots);

    loop {
        thread::sleep(Duration::from_millis(450));

        // Fast metadata check first (sub-millisecond)
        let cur_stat = stat_snapshot(&watch_roots);
        if cur_stat == last_stat_map {
            continue;
        }
        last_stat_map = cur_stat;

        // Verify actual content hashes & ignore auto-generated .webp sidecars
        let cur_hash = content_snapshot(&watch_roots);
        if cur_hash == last_hash_map || only_derived_webp(&last_hash_map, &cur_hash) {
            last_hash_map = cur_hash;
            continue;
        }

        if run_watch_cycle(&state, &mut last_shared).is_ok() {
            // Re-snapshot after build so newly generated .webp files in assets/ don't re-trigger
            let post_stat = stat_snapshot(&watch_roots);
            let post_hash = content_snapshot(&watch_roots);
            if only_derived_webp(&cur_hash, &post_hash) {
                last_stat_map = post_stat;
                last_hash_map = post_hash;
            } else {
                last_hash_map = cur_hash;
            }
        } else {
            last_hash_map = cur_hash;
        }
    }
}

fn collect_watch_roots(cfg: &ServerConfig) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for p in [
        &cfg.source_dir,
        &cfg.template_dir,
        &cfg.static_dir,
        &cfg.assets_dir,
    ] {
        if p.exists() && !v.iter().any(|existing: &PathBuf| p.starts_with(existing)) {
            v.push(p.clone());
        }
    }
    v
}

fn is_ignored_editor_file(name: &str) -> bool {
    name.starts_with('.')
        || name.ends_with('~')
        || (name.starts_with('#') && name.ends_with('#'))
        || name.ends_with(".swp")
        || name.ends_with(".swo")
        || name.ends_with(".swx")
        || name == "4913"
}

fn stat_snapshot(roots: &[PathBuf]) -> HashMap<PathBuf, (u128, u64)> {
    let mut map = HashMap::new();
    fn walk(p: &Path, map: &mut HashMap<PathBuf, (u128, u64)>) {
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
            return;
        };
        if is_ignored_editor_file(name) {
            return;
        }
        if p.is_dir() {
            if let Ok(entries) = fs::read_dir(p) {
                for e in entries.flatten() {
                    walk(&e.path(), map);
                }
            }
        } else if let Ok(md) = fs::metadata(p) {
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            map.insert(p.to_path_buf(), (mtime, md.len()));
        }
    }
    for r in roots {
        if r.is_dir() {
            if let Ok(entries) = fs::read_dir(r) {
                for e in entries.flatten() {
                    walk(&e.path(), &mut map);
                }
            }
        } else if r.is_file() {
            walk(r, &mut map);
        }
    }
    map
}

fn content_snapshot(roots: &[PathBuf]) -> HashMap<PathBuf, u64> {
    let mut map = HashMap::new();
    fn walk(p: &Path, map: &mut HashMap<PathBuf, u64>) {
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
            return;
        };
        if is_ignored_editor_file(name) {
            return;
        }
        if p.is_dir() {
            if let Ok(entries) = fs::read_dir(p) {
                for e in entries.flatten() {
                    walk(&e.path(), map);
                }
            }
        } else if let Ok(bytes) = fs::read(p) {
            map.insert(p.to_path_buf(), fnv1a(&bytes));
        }
    }
    for r in roots {
        if r.is_dir() {
            if let Ok(entries) = fs::read_dir(r) {
                for e in entries.flatten() {
                    walk(&e.path(), &mut map);
                }
            }
        } else if r.is_file() {
            walk(r, &mut map);
        }
    }
    map
}

fn only_derived_webp(pre: &HashMap<PathBuf, u64>, post: &HashMap<PathBuf, u64>) -> bool {
    // Every file from `pre` must still exist in `post` with identical hash
    for (p, h) in pre {
        if post.get(p) != Some(h) {
            return false;
        }
    }
    let post_lower: HashSet<String> = post
        .keys()
        .map(|p| p.to_string_lossy().to_ascii_lowercase())
        .collect();

    // Any newly added file in `post` must be a .webp with a matching .png/.jpg/.jpeg
    for p in post.keys() {
        if pre.contains_key(p) {
            continue;
        }
        let lower = p.to_string_lossy().to_ascii_lowercase();
        let Some(stem) = lower.strip_suffix(".webp") else {
            return false;
        };
        let has_src = post_lower.contains(&format!("{}.png", stem))
            || post_lower.contains(&format!("{}.jpg", stem))
            || post_lower.contains(&format!("{}.jpeg", stem));
        if !has_src {
            return false;
        }
    }
    true
}

fn compute_shared_fingerprint(source_dir: &Path) -> u64 {
    let mut files = Vec::new();
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    collect(&p, out);
                } else if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                    if ext == "wiki" || ext == "md" {
                        out.push(p);
                    }
                }
            }
        }
    }
    collect(source_dir, &mut files);
    files.sort();

    // Pass 1: find block types referenced by {{ list.[block.]<type> ... }}
    let mut want_blocks: HashSet<String> = HashSet::new();
    let mut contents: Vec<(String, String)> = Vec::with_capacity(files.len());

    for f in &files {
        let rel = f
            .strip_prefix(source_dir)
            .unwrap_or(f)
            .to_string_lossy()
            .into_owned();
        let text = fs::read_to_string(f).unwrap_or_default();
        let mut rem = text.as_str();
        while let Some(i1) = rem.find("{{") {
            let after = &rem[i1 + 2..];
            let Some(i2) = after.find("}}") else { break };
            let inner = after[..i2].trim();
            if let Some(rest) = inner.strip_prefix("list.") {
                let rest = rest.strip_prefix("block.").unwrap_or(rest);
                let btype: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                if !btype.is_empty() && btype != "pages" && btype != "block" {
                    let singular = btype.strip_suffix('s').unwrap_or(&btype).to_string();
                    want_blocks.insert(btype);
                    want_blocks.insert(singular);
                }
            }
            rem = &after[i2 + 2..];
        }
        contents.push((rel, text));
    }

    // Pass 2: hash file list + frontmatter + block.globals + listed blocks + all block IDs/contexts
    let mut buf = String::new();
    for (rel, _) in &contents {
        buf.push_str(rel);
        buf.push('\n');
    }

    for (rel, text) in &contents {
        let mut fm = 0u8;
        let mut g = 0u8;
        let mut cur_b = String::new();
        let mut nest = 0usize;

        for raw_line in text.lines() {
            let line = raw_line.trim_end_matches('\r');
            let trimmed = line.trim();

            if !cur_b.is_empty() {
                let check = trimmed.strip_prefix("::").map(|s| s.trim()).unwrap_or(trimmed);
                if check.starts_with("{{") && check.ends_with("}}") {
                    let inner = check[2..check.len() - 2].trim();
                    if inner == "block.end" {
                        if nest > 0 {
                            nest -= 1;
                        } else {
                            buf.push_str(rel);
                            buf.push_str(":b:");
                            buf.push_str(line);
                            buf.push('\n');
                            cur_b.clear();
                            continue;
                        }
                    } else if inner.starts_with("block.") {
                        nest += 1;
                    }
                }
                if want_blocks.contains(&cur_b)
                    || trimmed.starts_with(":: __id__:")
                    || trimmed.starts_with(":: __context__:")
                    || trimmed.starts_with(":: context:")
                    || trimmed.starts_with(":: term:")
                {
                    buf.push_str(rel);
                    buf.push_str(":b:");
                    buf.push_str(line);
                    buf.push('\n');
                }
                continue;
            }

            if fm < 2 && trimmed == "---" {
                fm += 1;
                continue;
            } else if fm == 1 {
                buf.push_str(rel);
                buf.push_str(":fm:");
                buf.push_str(line);
                buf.push('\n');
                continue;
            }

            if g == 0 && trimmed.starts_with("{{") && trimmed.ends_with("}}") {
                let inner = trimmed[2..trimmed.len() - 2].trim();
                if inner == "block.globals" {
                    g = 1;
                    continue;
                }
                if let Some(m) = inner.strip_prefix("block.") {
                    let bname = m.split_whitespace().next().unwrap_or("");
                    if !bname.is_empty() && bname != "comment" {
                        cur_b = bname.to_string();
                        nest = 0;
                        buf.push_str(rel);
                        buf.push_str(":b:");
                        buf.push_str(line);
                        buf.push('\n');
                        continue;
                    }
                }
            } else if g == 1 {
                if trimmed.starts_with("{{")
                    && trimmed.ends_with("}}")
                    && trimmed[2..trimmed.len() - 2].trim() == "block.end"
                {
                    g = 2;
                } else {
                    buf.push_str(rel);
                    buf.push_str(":g:");
                    buf.push_str(line);
                    buf.push('\n');
                }
            }
        }
    }

    fnv1a(buf.as_bytes())
}

fn load_shared_fingerprint(cache_dir: &Path) -> u64 {
    fs::read_to_string(cache_dir.join("watch_shared.hash"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn save_shared_fingerprint(cache_dir: &Path, hash: u64) {
    let _ = fs::write(cache_dir.join("watch_shared.hash"), format!("{}\n", hash));
}

fn run_watch_cycle(state: &AppState, last_shared: &mut u64) -> Result<(), ()> {
    let cfg = &state.cfg;
    let cur_shared = compute_shared_fingerprint(&cfg.source_dir);
    let need_full = *last_shared == 0 || cur_shared != *last_shared || !cfg.stage_dir.join("index.json").is_file();

    let mut cmd = Command::new(&cfg.builder_bin);
    cmd.arg("--wiki")
        .arg(&cfg.wiki_name)
        .arg("--wiki-dir")
        .arg(&cfg.wiki_dir)
        .arg("--source")
        .arg(&cfg.source_dir)
        .arg("--dist")
        .arg(&cfg.stage_dir)
        .arg("--templates")
        .arg(&cfg.template_dir)
        .arg("--static")
        .arg(&cfg.static_dir)
        .arg("--assets")
        .arg(&cfg.assets_dir)
        .arg("--cache")
        .arg(&cfg.cache_dir)
        .arg("--jobs")
        .arg(cfg.jobs.to_string());

    if cfg.config_tsv.is_file() {
        cmd.arg("--config").arg(&cfg.config_tsv);
    }
    if !cfg.domain.is_empty() {
        cmd.arg("--domain").arg(&cfg.domain);
    }
    if need_full {
        cmd.arg("--rebuild");
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("wiki:error: Failed to execute builder: {}", e);
            return Err(());
        }
    };

    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    if !output.status.success() {
        eprintln!("wiki:error: Build failed; open browser tabs will not reload.");
        return Err(());
    }

    *last_shared = cur_shared;
    save_shared_fingerprint(&cfg.cache_dir, cur_shared);

    if let Err(e) = sync_stage_to_serve(&cfg.stage_dir, &cfg.serve_dir) {
        eprintln!("wiki:error: Failed syncing stage dir: {}", e);
        return Err(());
    }

    state.reload_token.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

fn sync_stage_to_serve(stage: &Path, serve: &Path) -> std::io::Result<()> {
    fs::create_dir_all(serve)?;
    let mut staged_rel: HashSet<PathBuf> = HashSet::new();

    fn copy_changed(
        stage_root: &Path,
        cur_stage: &Path,
        serve_root: &Path,
        staged_rel: &mut HashSet<PathBuf>,
    ) -> std::io::Result<()> {
        for entry in fs::read_dir(cur_stage)? {
            let entry = entry?;
            let sp = entry.path();
            let rel = sp.strip_prefix(stage_root).unwrap_or(&sp).to_path_buf();
            if rel.starts_with(".cache") {
                continue;
            }
            let dp = serve_root.join(&rel);
            if sp.is_dir() {
                fs::create_dir_all(&dp)?;
                copy_changed(stage_root, &sp, serve_root, staged_rel)?;
            } else {
                let sp_bytes = fs::read(&sp)?;
                let same = fs::read(&dp).map(|dp_bytes| dp_bytes == sp_bytes).unwrap_or(false);
                if !same {
                    if let Some(parent) = dp.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let tmp = dp.with_extension("watch-tmp");
                    fs::write(&tmp, &sp_bytes)?;
                    fs::rename(&tmp, &dp)?;
                }
                staged_rel.insert(rel);
            }
        }
        Ok(())
    }

    copy_changed(stage, stage, serve, &mut staged_rel)?;

    // Prune deleted files & empty directories from serve_dir
    fn prune(serve_root: &Path, cur: &Path, staged_rel: &HashSet<PathBuf>) -> std::io::Result<bool> {
        let mut is_empty = true;
        for entry in fs::read_dir(cur)? {
            let entry = entry?;
            let p = entry.path();
            let rel = p.strip_prefix(serve_root).unwrap_or(&p);
            if rel == Path::new("_reload") || rel.starts_with(".cache") {
                is_empty = false;
                continue;
            }
            if p.is_dir() {
                if prune(serve_root, &p, staged_rel)? {
                    let _ = fs::remove_dir(&p);
                } else {
                    is_empty = false;
                }
            } else if !staged_rel.contains(rel) {
                let _ = fs::remove_file(&p);
            } else {
                is_empty = false;
            }
        }
        Ok(is_empty)
    }

    let _ = prune(serve, serve, &staged_rel);
    Ok(())
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn open_in_browser(url: &str) {
    for cmd in ["xdg-open", "open"] {
        if Command::new(cmd)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            break;
        }
    }
}

fn parse_cli() -> ServerConfig {
    let args: Vec<String> = env::args().collect();
    let mut port: u16 = 8080;
    let mut serve_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut watch = false;
    let mut open_browser = false;
    let mut builder_bin = PathBuf::new();
    let mut wiki_name = String::new();
    let mut wiki_dir = PathBuf::new();
    let mut source_dir = PathBuf::new();
    let mut template_dir = PathBuf::new();
    let mut static_dir = PathBuf::new();
    let mut assets_dir = PathBuf::new();
    let mut cache_dir = PathBuf::new();
    let mut stage_dir = PathBuf::new();
    let mut config_tsv = PathBuf::new();
    let mut domain = String::new();
    let mut jobs = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-p" | "--port" if i + 1 < args.len() => {
                port = args[i + 1].parse().unwrap_or(8080);
                i += 2;
            }
            "-d" | "--dir" | "--dist" if i + 1 < args.len() => {
                serve_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "-w" | "--watch" => {
                watch = true;
                i += 1;
            }
            "--open" => {
                open_browser = true;
                i += 1;
            }
            "--builder" if i + 1 < args.len() => {
                builder_bin = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--wiki" if i + 1 < args.len() => {
                wiki_name = args[i + 1].clone();
                i += 2;
            }
            "--wiki-dir" if i + 1 < args.len() => {
                wiki_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--source" if i + 1 < args.len() => {
                source_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--templates" if i + 1 < args.len() => {
                template_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--static" if i + 1 < args.len() => {
                static_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--assets" if i + 1 < args.len() => {
                assets_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--cache" if i + 1 < args.len() => {
                cache_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--stage" if i + 1 < args.len() => {
                stage_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--config" if i + 1 < args.len() => {
                config_tsv = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--domain" if i + 1 < args.len() => {
                domain = args[i + 1].clone();
                i += 2;
            }
            "-j" | "--jobs" if i + 1 < args.len() => {
                jobs = args[i + 1].parse().unwrap_or(4).max(1);
                i += 2;
            }
            arg if !arg.starts_with('-') && arg.bytes().all(|b| b.is_ascii_digit()) => {
                // Backward compatibility: `bin/server 8080`
                port = arg.parse().unwrap_or(8080);
                i += 1;
            }
            _ => i += 1,
        }
    }

    if cache_dir.as_os_str().is_empty() {
        cache_dir = serve_dir.join(".cache");
    }
    if stage_dir.as_os_str().is_empty() {
        stage_dir = cache_dir.join("dist");
    }
    if builder_bin.as_os_str().is_empty() && !wiki_dir.as_os_str().is_empty() {
        builder_bin = wiki_dir.join("bin/builder");
    }

    ServerConfig {
        port,
        serve_dir,
        watch,
        open_browser,
        builder_bin,
        wiki_name,
        wiki_dir,
        source_dir,
        template_dir,
        static_dir,
        assets_dir,
        cache_dir,
        stage_dir,
        config_tsv,
        domain,
        jobs,
    }
}
