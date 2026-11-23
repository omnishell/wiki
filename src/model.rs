use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::RwLock;

#[derive(Clone, Debug)]
pub struct Config {
    pub wiki_name: String,
    pub wiki_env: String,
    pub wiki_dir: PathBuf,
    pub source_dir: PathBuf,
    pub build_dir: PathBuf,
    pub template_dir: PathBuf,
    pub static_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub domain: String,
    pub target_page: String,
    pub jobs: usize,
    pub force_rebuild: bool,
    pub build_year: String,
    pub build_now: String,
    pub build_date_iso: String,
}

#[derive(Clone, Debug, Default)]
pub struct OmniEnv {
    pub custom_blocks: HashMap<String, String>,
    pub authors: HashMap<String, (String, String)>, // id -> (name, url)
    pub templates: HashMap<String, String>,
    pub featured: Vec<String>,
    pub external_refs: HashMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct PageMeta {
    pub slug: String,
    pub file_path: PathBuf,
    pub mtime: u64,
    pub size: u64,
    pub hash: u64,
    pub implicit_weight: String,
    pub title: String,
    pub weight: String,
    pub template: String,
    pub tags: String,
    pub thumb: String,
    pub related: String,
    pub author: String,
    pub partials: String,
    pub vars: HashMap<String, String>,
    pub block_types: HashSet<String>,
    pub block_title_keys: HashMap<String, String>,
    pub dirty: bool,
}

pub struct WikiContext {
    pub cfg: Config,
    pub omni: OmniEnv,
    pub pages: Vec<String>,
    pub global_pages: Vec<String>,
    pub nested_pages: Vec<String>,
    pub sorted_pages_leaf_first: Vec<String>,
    pub page_meta: HashMap<String, PageMeta>,
    pub dir_weights: HashMap<String, String>,
    pub is_dir_page: HashSet<String>,
    pub dir_best_page: HashMap<String, String>,
    pub refs: HashMap<String, String>,
    pub latex_cache: RwLock<HashMap<String, String>>,
    pub layout_cache: HashMap<PathBuf, String>,
    pub wiki_menu_html: String,
    pub wiki_featured_html: String,
    pub search_js: String,
}

#[derive(Default)]
pub struct PageRenderOutput {
    pub slug: String,
    pub search_entries: BTreeMap<String, String>,
    pub title_keys: HashMap<String, String>,
}

pub struct RenderState<'a> {
    pub ctx: &'a WikiContext,
    pub pagename: String,
    pub file_path: PathBuf,
    pub current_dir: String,
    pub page_vars: HashMap<String, String>,
    pub toc_html: String,
    pub page_table_count: usize,
    pub page_diagram_count: usize,
    pub search_index_map: BTreeMap<String, String>,
    pub list_html_map: HashMap<String, String>,
    pub page_title_keys: HashMap<String, String>,
}

pub fn slugify(input: &str) -> String {
    let lower = input.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    for ch in lower.chars() {
        let mapped = match ch {
            ' ' => '-',
            'á' | 'à' | 'ã' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'õ' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '/' || c == '-' => c,
            _ => continue,
        };
        out.push(mapped);
    }
    if out.starts_with('-') && out.chars().all(|c| matches!(c, '-' | 'n' | 'e' | 'E')) {
        return String::new();
    }
    out
}

pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

pub fn strip_quotes(s: &str) -> &str {
    let t = s.trim();
    if t.len() >= 2 {
        let bytes = t.as_bytes();
        if (bytes[0] == b'"' && bytes[t.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[t.len() - 1] == b'\'')
        {
            return &t[1..t.len() - 1];
        }
    }
    t
}

pub fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn parse_kv_args(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        while i < n && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= n {
            break;
        }
        let k_start = i;
        while i < n && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        if i >= n || bytes[i] != b'=' {
            while i < n && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            continue;
        }
        let key = s[k_start..i].to_string();
        i += 1; // skip '='
        if i >= n {
            out.push((key, String::new()));
            break;
        }
        let val = if bytes[i] == b'"' || bytes[i] == b'\'' {
            let q = bytes[i];
            i += 1;
            let v_start = i;
            while i < n && bytes[i] != q {
                i += 1;
            }
            let v = s[v_start..i].to_string();
            if i < n && bytes[i] == q {
                i += 1;
            }
            v
        } else {
            let v_start = i;
            while i < n && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            s[v_start..i].to_string()
        };
        out.push((key, val));
    }
    out
}

pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut star_ti) = (None, 0usize);

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

pub fn split_num_prefix(s: &str) -> Option<(&str, &str)> {
    if let Some(dot) = s.find('.') {
        let left = &s[..dot];
        let right = &s[dot + 1..];
        if !left.is_empty() && left.bytes().all(|b| b.is_ascii_digit()) && !right.is_empty() {
            return Some((left, right));
        }
    }
    None
}

pub fn sort_page_hierarchy(
    items: &[String],
    delim: char,
    mode: &str,
    leaf_weights: Option<&HashMap<String, String>>,
    page_weights: &HashMap<String, String>,
    root_slug: &str,
    dir_weights: Option<&HashMap<String, String>>,
) -> Vec<String> {
    if items.is_empty() {
        return Vec::new();
    }
    let (leaf_type, dir_type) = if mode == "dir_first" {
        ('1', '0')
    } else {
        ('0', '1')
    };

    let mut keyed: Vec<(String, String)> = Vec::with_capacity(items.len());
    for item in items {
        let parts: Vec<&str> = item.split(delim).collect();
        let mut sort_key = String::new();
        let mut cum_path = String::new();

        for (i, &part) in parts.iter().enumerate() {
            if cum_path.is_empty() {
                cum_path.push_str(part);
            } else {
                cum_path.push(delim);
                cum_path.push_str(part);
            }
            let (typ, mut w) = if i == parts.len() - 1 {
                let lw = if let Some(lw_map) = leaf_weights {
                    lw_map.get(item).map(|s| s.as_str()).unwrap_or("9999999")
                } else {
                    page_weights
                        .get(&cum_path)
                        .map(|s| s.as_str())
                        .unwrap_or("9999999")
                };
                (leaf_type, lw.to_string())
            } else {
                let dir_slug_key = if delim == '/' {
                    let rep = cum_path.replace('/', ".");
                    let combined = if !root_slug.is_empty() {
                        format!("{}.{}", root_slug, rep)
                    } else {
                        rep
                    };
                    combined.trim_start_matches('.').to_string()
                } else {
                    cum_path.clone()
                };
                let mut dw = page_weights
                    .get(&dir_slug_key)
                    .map(|s| s.as_str())
                    .unwrap_or("9999999")
                    .to_string();
                if dw == "9999999" {
                    if let Some(dw_map) = dir_weights {
                        if let Some(v) = dw_map.get(&dir_slug_key) {
                            dw = v.clone();
                        }
                    }
                }
                if dw == "9999999" {
                    if part.bytes().all(|b| b.is_ascii_digit()) && !part.is_empty() {
                        dw = part.to_string();
                    } else if let Some((num, _)) = split_num_prefix(part) {
                        dw = num.to_string();
                    }
                }
                (dir_type, dw)
            };
            if w.is_empty() || !w.bytes().all(|b| b.is_ascii_digit()) {
                w = "9999999".to_string();
            }
            let padded = format!("{:0>7}", w);
            let padded_tail = &padded[padded.len().saturating_sub(7)..];
            sort_key.push('/');
            sort_key.push(typ);
            sort_key.push('_');
            sort_key.push_str(padded_tail);
            sort_key.push('_');
            sort_key.push_str(part);
        }
        keyed.push((sort_key, item.clone()));
    }
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.into_iter().map(|(_, v)| v).collect()
}

pub fn scan_page_metadata_and_refs(
    content: &str,
    slug: &str,
    meta: &mut PageMeta,
    out_refs: &mut Vec<(String, String, String)>, // (block_type, ref_key, anchor_id)
) {
    let mut in_fm = false;
    let mut fm_checked = false;
    let mut in_globals = false;
    let mut in_block = String::new();
    let mut nest: usize = 0;

    let mut b_id = String::new();
    let mut b_ctx = String::new();
    let mut b_entry = String::new();
    let mut first_v = String::new();

    let mut in_sub_diag: usize = 0;
    let mut sub_id = String::new();
    let mut sub_ctx = String::new();

    meta.block_types.insert("page".to_string());

    for raw_line in content.lines() {
        let line = raw_line.trim_end_matches('\r');
        let l_trim = line.trim_start();

        if !fm_checked {
            fm_checked = true;
            if line == "---" {
                in_fm = true;
                continue;
            }
        }
        if in_fm {
            if line == "---" {
                in_fm = false;
                continue;
            }
            if let Some(idx) = l_trim.find(':') {
                let k = &l_trim[..idx];
                if !k.is_empty()
                    && k.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'.')
                {
                    let v = strip_quotes(l_trim[idx + 1..].trim()).to_string();
                    meta.vars.insert(k.to_string(), v.clone());
                    meta.vars.insert(format!("page.{}", k), v.clone());
                    if k == "weighted" {
                        meta.vars.insert("weight".to_string(), v.clone());
                        meta.vars.insert("page.weight".to_string(), v);
                    }
                }
            }
            continue;
        }

        let mut check_trim = l_trim;
        if !in_block.is_empty() && check_trim.starts_with("::") {
            let after = check_trim[2..].trim_start();
            if after.starts_with("{{") {
                check_trim = after;
            }
        }

        if check_trim.starts_with("{{") && check_trim.trim_end().ends_with("}}") {
            let r_trim = check_trim.trim_end();
            let inner = r_trim[2..r_trim.len() - 2].trim();
            if inner == "block.end" {
                if nest > 0 {
                    if in_sub_diag == nest {
                        if !sub_id.is_empty() {
                            let eff_ctx = if !sub_ctx.is_empty() { &sub_ctx } else { &b_ctx };
                            let mut ref_key = slugify(&sub_id);
                            if !eff_ctx.is_empty() && !sub_id.contains('.') {
                                ref_key = slugify(&format!("{}.{}", eff_ctx, sub_id));
                            }
                            let anchor_slug = ref_key.replace(['/', '.'], "-");
                            out_refs.push((
                                "diagram".to_string(),
                                ref_key,
                                format!("block-diagram-{}", anchor_slug),
                            ));
                        }
                        in_sub_diag = 0;
                    }
                    nest -= 1;
                    continue;
                }
                if !in_block.is_empty() && in_block != "globals" && in_block != "comment" {
                    let raw_id = if !b_id.is_empty() {
                        &b_id
                    } else if !b_entry.is_empty() {
                        &b_entry
                    } else {
                        &first_v
                    };
                    if !raw_id.is_empty() && (in_block != "diagram" || !b_id.is_empty()) {
                        let mut ref_key = slugify(raw_id);
                        if !b_ctx.is_empty() && !raw_id.contains('.') {
                            ref_key = slugify(&format!("{}.{}", b_ctx, raw_id));
                        }
                        let anchor_slug = ref_key.replace(['/', '.'], "-");
                        let anchor = format!("block-{}-{}", in_block, anchor_slug);
                        out_refs.push((in_block.clone(), ref_key, anchor));
                    }
                }
                in_globals = false;
                in_block.clear();
                continue;
            } else if let Some(bname) = inner.strip_prefix("block.") {
                let clean_bname = bname.split_whitespace().next().unwrap_or("");
                if !in_block.is_empty() || in_globals {
                    nest += 1;
                    if inner == "block.diagram" {
                        in_sub_diag = nest;
                        sub_id.clear();
                        sub_ctx.clear();
                    }
                    continue;
                }
                if !clean_bname.is_empty()
                    && clean_bname
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
                {
                    if clean_bname == "globals" {
                        in_globals = true;
                    } else {
                        in_block = clean_bname.to_string();
                        b_id.clear();
                        b_ctx.clear();
                        b_entry.clear();
                        first_v.clear();
                        if clean_bname != "comment" {
                            meta.block_types.insert(clean_bname.to_string());
                        }
                        if clean_bname == "table" {
                            meta.block_title_keys
                                .insert("table".to_string(), "caption".to_string());
                        }
                    }
                    continue;
                }
            }
        }

        if in_globals {
            if let Some(after_col) = l_trim.strip_prefix("::") {
                let after_col = after_col.trim_start();
                if let Some(idx) = after_col.find(':') {
                    let k = &after_col[..idx];
                    if !k.is_empty()
                        && k.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'.')
                    {
                        let mut v = strip_quotes(after_col[idx + 1..].trim()).to_string();
                        if k == "thumb" && v.starts_with("./") {
                            let cur_dir = slug.rfind('.').map(|i| &slug[..i]).unwrap_or("");
                            let dir_path = cur_dir.replace('.', "/");
                            let stem = Path::new(&v[2..])
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or(&v[2..]);
                            let parent = Path::new(&v[2..])
                                .parent()
                                .and_then(|p| p.to_str())
                                .unwrap_or("");
                            let rel_webp = if parent.is_empty() {
                                format!("{}.webp", stem)
                            } else {
                                format!("{}/{}.webp", parent, stem)
                            };
                            if dir_path.is_empty() {
                                v = format!("/_assets/{}", rel_webp);
                            } else {
                                v = format!("/_assets/{}/{}", dir_path, rel_webp);
                            }
                        }
                        meta.vars.insert(k.to_string(), v.clone());
                        meta.vars.insert(format!("page.{}", k), v.clone());
                        if k == "weighted" {
                            meta.vars.insert("weight".to_string(), v.clone());
                            meta.vars.insert("page.weight".to_string(), v);
                        }
                    }
                }
            }
            continue;
        }

        if in_sub_diag > 0 && nest == in_sub_diag {
            let mut s = l_trim;
            while let Some(rest) = s.strip_prefix("::") {
                s = rest.trim_start();
            }
            if let Some(idx) = s.find(':') {
                let k = s[..idx].trim();
                let v = strip_quotes(s[idx + 1..].trim());
                if k == "__id__" || k == "id" {
                    sub_id = v.to_string();
                } else if k == "__context__" || k == "context" {
                    sub_ctx = v.to_string();
                }
            }
            continue;
        }

        if !in_block.is_empty() && nest == 0 && l_trim.starts_with("::") {
            let after_col = l_trim[2..].trim_start();
            if let Some(idx) = after_col.find(':') {
                let k = after_col[..idx].trim();
                if !k.is_empty()
                    && k.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'.')
                {
                    let v = strip_quotes(after_col[idx + 1..].trim()).to_string();
                    if k == "__id__" || (in_block == "diagram" && k == "id") {
                        b_id = v;
                    } else if k == "__context__" || k == "context" {
                        b_ctx = v;
                    } else if k == "__entry__" {
                        b_entry = v;
                    } else if k != "__refs__" && in_block != "diagram" {
                        if first_v.is_empty() {
                            first_v = v;
                        }
                        meta.block_title_keys
                            .entry(in_block.clone())
                            .or_insert_with(|| k.to_string());
                    }
                }
            }
        }
    }

    meta.title = meta
        .vars
        .get("page.title")
        .or_else(|| meta.vars.get("title"))
        .cloned()
        .unwrap_or_default();
    meta.template = meta
        .vars
        .get("page.template")
        .or_else(|| meta.vars.get("template"))
        .cloned()
        .unwrap_or_default();
    meta.tags = meta
        .vars
        .get("page.tags")
        .or_else(|| meta.vars.get("tags"))
        .cloned()
        .unwrap_or_default();
    if !meta.tags.is_empty() {
        meta.block_types.insert("tag".to_string());
    }
    meta.thumb = meta
        .vars
        .get("page.thumb")
        .or_else(|| meta.vars.get("thumb"))
        .cloned()
        .unwrap_or_default();
    meta.related = meta
        .vars
        .get("page.related")
        .or_else(|| meta.vars.get("related"))
        .cloned()
        .unwrap_or_default();
    meta.author = meta
        .vars
        .get("page.author")
        .or_else(|| meta.vars.get("author"))
        .cloned()
        .unwrap_or_default();
    meta.partials = meta
        .vars
        .get("page.partials")
        .or_else(|| meta.vars.get("partials"))
        .cloned()
        .unwrap_or_default();

    let raw_w = meta
        .vars
        .get("page.weight")
        .or_else(|| meta.vars.get("weight"))
        .cloned()
        .unwrap_or_default();

    meta.weight = if !raw_w.is_empty() && raw_w.bytes().all(|b| b.is_ascii_digit()) {
        raw_w
    } else if !meta.implicit_weight.is_empty() {
        meta.implicit_weight.clone()
    } else {
        "9999999".to_string()
    };
}

pub fn render_latex_expr(wiki_dir: &Path, expr: &str, inline: bool) -> Option<String> {
    let latex_bin = wiki_dir.join("bin/latex");
    if !latex_bin.exists() {
        return None;
    }
    let mut cmd = Command::new(latex_bin);
    cmd.arg("--stdout").arg("--dpr").arg("0.4");
    if inline {
        cmd.arg("--inline");
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().ok()?;
    let input = if inline {
        format!("{}\\rule{{0pt}}{{0pt}}", expr)
    } else {
        expr.to_string()
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() && output.stdout.is_empty() {
        return None;
    }
    let mut svg = String::from_utf8_lossy(&output.stdout).into_owned();
    if svg.trim().is_empty() {
        return None;
    }
    svg = svg.replace("rgba(0,0,0,1)", "var(--fg)");
    if !inline {
        return Some(svg);
    }
    let (min_y, height) = extract_svg_viewbox(&svg).unwrap_or((0.0, 0.0));
    let align = if let Some(rect_y) = extract_baseline_rect_y(&svg) {
        format!("{}pt - ({}pt + {}pt) - 1px", rect_y, min_y, height)
    } else {
        "-10px".to_string()
    };
    Some(format!(
        "<span class=\"latex-inline\" style=\"vertical-align: calc({});\">{}</span>",
        align, svg
    ))
}

pub fn extract_svg_viewbox(svg: &str) -> Option<(f64, f64)> {
    let idx = svg.find("viewBox=")?;
    let rest = &svg[idx + 8..];
    let q = rest.as_bytes().first().copied()?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    let end = rest[1..].find(q as char)?;
    let inner = &rest[1..1 + end];
    let nums: Vec<f64> = inner
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if nums.len() >= 4 {
        Some((nums[1], nums[3]))
    } else {
        None
    }
}

pub fn extract_svg_viewbox_width(svg: &str) -> Option<i64> {
    let idx = svg.find("viewBox=")?;
    let rest = &svg[idx + 8..];
    let q = rest.as_bytes().first().copied()?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    let end = rest[1..].find(q as char)?;
    let inner = &rest[1..1 + end];
    let nums: Vec<f64> = inner
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if nums.len() >= 3 {
        Some(nums[2] as i64)
    } else {
        None
    }
}

fn extract_baseline_rect_y(svg: &str) -> Option<String> {
    let mut search = svg;
    let mut last_y = None;
    while let Some(pos) = search.find("<rect ") {
        let sub = &search[pos..];
        let tag_end = sub.find('>')?;
        let tag = &sub[..tag_end];
        if tag.contains("width=\"0\"") || tag.contains("width='0'") {
            if let Some(y_idx) = tag.find(" y=") {
                let yr = &tag[y_idx + 3..];
                let q = yr.as_bytes().first().copied()?;
                if q == b'"' || q == b'\'' {
                    if let Some(e) = yr[1..].find(q as char) {
                        last_y = Some(yr[1..1 + e].to_string());
                    }
                }
            }
        }
        search = &sub[tag_end + 1..];
    }
    last_y
}

pub fn get_or_render_latex(ctx: &WikiContext, expr: &str, inline: bool) -> Option<String> {
    let key = if inline {
        format!("inline:{}", expr)
    } else {
        format!("block:{}", expr)
    };
    if let Ok(guard) = ctx.latex_cache.read() {
        if let Some(v) = guard.get(&key) {
            return Some(v.clone());
        }
    }
    let rendered = render_latex_expr(&ctx.cfg.wiki_dir, expr, inline)?;
    if let Ok(mut guard) = ctx.latex_cache.write() {
        guard.insert(key, rendered.clone());
    }
    Some(rendered)
}

pub fn resolve_wiki_ref(ctx: &WikiContext, topic: &str, ref_key: &str, label: &str) -> String {
    let key1 = format!("{}::{}", topic, ref_key);
    let key2 = format!(
        "{}::{}::{}::{}",
        ctx.cfg.wiki_name, ctx.cfg.wiki_env, topic, ref_key
    );
    let target = ctx
        .refs
        .get(&key1)
        .or_else(|| ctx.refs.get(&key2))
        .cloned();

    let Some(t) = target else {
        return format!("<span class=\"wiki-ref-err\">{}</span>", label);
    };

    let href = if t.starts_with("http://") || t.starts_with("https://") {
        t
    } else if let Some(colon) = t.find(':') {
        let ipath = t[..colon].replace('.', "/");
        let marker = &t[colon + 1..];
        if marker.is_empty() {
            format!("/{}.html", ipath)
        } else {
            format!("/{}.html#{}", ipath, marker)
        }
    } else {
        format!("/{}.html", t.replace('.', "/"))
    };
    format!(
        "<a class=\"wiki-topic-{}\" href=\"{}\">{}</a>",
        topic, href, label
    )
}

pub fn get_style_attr(b_width: &str, b_style: &str) -> String {
    let mut inline_style = String::new();
    if !b_width.is_empty() {
        if b_width.contains('=') {
            for tok in b_width.split(',') {
                let tok = tok.trim();
                if let Some(eq) = tok.find('=') {
                    let prop = &tok[..eq];
                    let mut val = tok[eq + 1..].to_string();
                    if !val.is_empty() && val.bytes().all(|b| b.is_ascii_digit()) {
                        val.push_str("px");
                    }
                    match prop {
                        "max" => inline_style.push_str(&format!("max-width:{};", val)),
                        "min" => inline_style.push_str(&format!("min-width:{};", val)),
                        "width" => inline_style.push_str(&format!("width:{};", val)),
                        _ => {}
                    }
                }
            }
        } else {
            let mut val = b_width.to_string();
            if val.bytes().all(|b| b.is_ascii_digit()) {
                val.push_str("px");
            }
            inline_style.push_str(&format!("width:{};", val));
        }
    }
    if !b_style.is_empty() {
        inline_style.push_str(b_style);
        if !inline_style.ends_with(';') {
            inline_style.push(';');
        }
    }
    if inline_style.is_empty() {
        String::new()
    } else {
        format!(" style=\"{}\"", inline_style)
    }
}

pub fn load_omni_env(path: &Path) -> OmniEnv {
    let mut env = OmniEnv::default();
    let Ok(text) = fs::read_to_string(path) else {
        return env;
    };
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let Some(kind) = parts.next() else { continue };
        match kind {
            "BLOCK" => {
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    env.custom_blocks.insert(k.to_string(), v.to_string());
                }
            }
            "AUTHOR" => {
                if let (Some(id), Some(data)) = (parts.next(), parts.next()) {
                    let name = extract_quoted_field(data, "name:").unwrap_or_default();
                    let url = extract_quoted_field(data, "url:").unwrap_or_default();
                    env.authors.insert(id.to_string(), (name, url));
                }
            }
            "TMPL" => {
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    env.templates.insert(k.to_string(), v.to_string());
                }
            }
            "FEATURED" => {
                if let Some(rest) = parts.next() {
                    for item in rest.split_whitespace() {
                        if !item.is_empty() {
                            env.featured.push(item.to_string());
                        }
                    }
                }
            }
            "REF" => {
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    env.external_refs.insert(k.to_string(), v.to_string());
                }
            }
            _ => {}
        }
    }
    env
}

fn extract_quoted_field(s: &str, key: &str) -> Option<String> {
    let idx = s.find(key)?;
    let rest = &s[idx + key.len()..];
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else {
        let end = rest.find(',').unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}
