mod blocks;
mod diagram;
mod model;
mod parser;
mod template;

use model::{
    fnv1a_hash, json_escape, load_omni_env, render_latex_expr, scan_page_metadata_and_refs,
    slugify, sort_page_hierarchy, split_num_prefix, Config, PageMeta, PageRenderOutput,
    RenderState, WikiContext,
};
use parser::parse_page_body;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, RwLock};
use std::time::UNIX_EPOCH;
use template::{build_default_menu_html, build_featured_html, render_template};

fn main() {
    let cfg = parse_cli();
    let _ = fs::create_dir_all(&cfg.cache_dir);
    let _ = fs::create_dir_all(&cfg.build_dir);

    let omni = if let Some(cfg_file) = find_arg_val("--config") {
        load_omni_env(Path::new(&cfg_file))
    } else {
        Default::default()
    };

    let mut force_rebuild = cfg.force_rebuild;

    // 1. Check template + builder binary content fingerprint
    let tmpl_hash = compute_tree_fingerprint(&[
        cfg.template_dir.clone(),
        env::current_exe().unwrap_or_default(),
    ]);
    let tmpl_cache_file = cfg.cache_dir.join("rust_tmpl.hash");
    let old_tmpl_hash = fs::read_to_string(&tmpl_cache_file).unwrap_or_default();
    if tmpl_hash != old_tmpl_hash.trim() {
        force_rebuild = true;
        let _ = fs::write(&tmpl_cache_file, format!("{}\n", tmpl_hash));
    }

    if cfg.target_page.is_empty() && force_rebuild {
        eprintln!(
            "wiki:info: Cleaning and preparing environment for '{}'...",
            cfg.wiki_name
        );
        if let Ok(entries) = fs::read_dir(&cfg.build_dir) {
            let cache_name = cfg.cache_dir.file_name();
            for entry in entries.flatten() {
                if Some(entry.file_name().as_os_str()) != cache_name {
                    let p = entry.path();
                    if p.is_dir() {
                        let _ = fs::remove_dir_all(p);
                    } else {
                        let _ = fs::remove_file(p);
                    }
                }
            }
        }
        let _ = fs::remove_file(cfg.cache_dir.join("rust_meta.cache"));
    }

    // 2. Sync _static only if static fingerprint changed or force_rebuild
    sync_static_if_needed(&cfg, force_rebuild);

    // 3. Walk SOURCEDIR for *.wiki and *.md files
    let mut source_files: Vec<PathBuf> = Vec::new();
    collect_wiki_files(&cfg.source_dir, &mut source_files);
    source_files.sort();

    // Load cached metadata & refs if available
    let meta_cache_path = cfg.cache_dir.join("rust_meta.cache");
    let cached_entries = if !force_rebuild {
        load_meta_cache(&meta_cache_path)
    } else {
        HashMap::new()
    };

    let mut pages: Vec<String> = Vec::with_capacity(source_files.len());
    let mut global_pages: Vec<String> = Vec::new();
    let mut nested_pages: Vec<String> = Vec::new();
    let mut page_meta: HashMap<String, PageMeta> = HashMap::with_capacity(source_files.len());
    let mut dir_weights: HashMap<String, String> = HashMap::new();
    let mut refs: HashMap<String, String> = omni.external_refs.clone();
    let mut search_block_types: BTreeSet<String> = BTreeSet::new();
    let mut block_title_keys: HashMap<String, String> = HashMap::new();
    search_block_types.insert("page".to_string());

    let mut dirty_count = 0usize;
    let mut found_paths: HashSet<String> = HashSet::with_capacity(source_files.len());

    for file_path in &source_files {
        let path_str = file_path.to_string_lossy().into_owned();
        found_paths.insert(path_str.clone());

        let (mtime, size) = file_mtime_size(file_path);
        let rel = file_path.strip_prefix(&cfg.source_dir).unwrap_or(file_path);
        let (clean_slug, implicit_weight) = derive_slug_and_dir_weights(rel, &mut dir_weights);

        let out_html = cfg
            .build_dir
            .join(format!("{}.html", clean_slug.replace('.', "/")));
        let idx_cache = cfg.cache_dir.join(format!("{}_rust.idx", clean_slug));

        let mut pm = PageMeta {
            slug: clean_slug.clone(),
            file_path: file_path.clone(),
            mtime,
            size,
            implicit_weight,
            dirty: true,
            ..Default::default()
        };

        let mut used_cache = false;
        if let Some(ce) = cached_entries.get(&path_str) {
            if ce.mtime == mtime && ce.size == size && out_html.is_file() && idx_cache.is_file() {
                pm.hash = ce.hash;
                pm.title = ce.title.clone();
                pm.weight = ce.weight.clone();
                pm.template = ce.template.clone();
                pm.tags = ce.tags.clone();
                pm.thumb = ce.thumb.clone();
                pm.related = ce.related.clone();
                pm.author = ce.author.clone();
                pm.partials = ce.partials.clone();
                pm.vars = ce.vars.clone();
                pm.block_types = ce.block_types.clone();
                pm.block_title_keys = ce.block_title_keys.clone();
                pm.dirty = false;
                for (btype, rkey, anc) in &ce.refs {
                    refs.insert(
                        format!("{}::{}", btype, rkey),
                        format!("{}:{}", clean_slug, anc),
                    );
                    refs.insert(
                        format!("{}::{}::{}::{}", cfg.wiki_name, cfg.wiki_env, btype, rkey),
                        format!("{}:{}", clean_slug, anc),
                    );
                }
                used_cache = true;
            }
        }

        if !used_cache {
            let content = fs::read_to_string(file_path).unwrap_or_default();
            pm.hash = fnv1a_hash(content.as_bytes());
            let mut page_refs = Vec::new();
            scan_page_metadata_and_refs(&content, &clean_slug, &mut pm, &mut page_refs);

            if let Some(ce) = cached_entries.get(&path_str) {
                if !force_rebuild
                    && ce.hash == pm.hash
                    && out_html.is_file()
                    && idx_cache.is_file()
                {
                    pm.dirty = false;
                }
            }
            if pm.dirty {
                dirty_count += 1;
            }
            for (btype, rkey, anc) in page_refs {
                refs.insert(
                    format!("{}::{}", btype, rkey),
                    format!("{}:{}", clean_slug, anc),
                );
                refs.insert(
                    format!("{}::{}::{}::{}", cfg.wiki_name, cfg.wiki_env, btype, rkey),
                    format!("{}:{}", clean_slug, anc),
                );
            }
        }

        for bt in &pm.block_types {
            search_block_types.insert(bt.clone());
        }
        for (bk, bv) in &pm.block_title_keys {
            block_title_keys
                .entry(bk.clone())
                .or_insert_with(|| bv.clone());
        }

        if clean_slug.contains('.') {
            nested_pages.push(clean_slug.clone());
        } else {
            global_pages.push(clean_slug.clone());
        }
        pages.push(clean_slug.clone());
        page_meta.insert(clean_slug, pm);
    }

    let deleted_count = cached_entries
        .keys()
        .filter(|k| !found_paths.contains(*k))
        .count();

    // 4. Instantaneous short-circuit if nothing changed!
    if !force_rebuild
        && dirty_count == 0
        && deleted_count == 0
        && cfg.target_page.is_empty()
        && cfg.build_dir.join("index.json").is_file()
        && cfg.build_dir.join("defs.json").is_file()
    {
        eprintln!(
            "wiki:success: Wiki '{}' is up to date (no changes detected).",
            cfg.wiki_name
        );
        return;
    }

    // 5. Sync & convert WebP assets only if assets changed (skip on single-page build)
    if cfg.target_page.is_empty() {
        sync_assets_if_needed(&cfg, force_rebuild);
    }

    // 6. Compute directory hierarchy & best pages
    let mut page_weights_map: HashMap<String, String> = dir_weights.clone();
    for (slug, pm) in &page_meta {
        if !page_weights_map.contains_key(slug) || pm.weight != "9999999" {
            page_weights_map.insert(slug.clone(), pm.weight.clone());
        }
    }

    let mut is_dir_page: HashSet<String> = HashSet::new();
    let mut dir_best_page: HashMap<String, String> = HashMap::new();
    let mut dir_best_weight: HashMap<String, u64> = HashMap::new();

    for p in &pages {
        let w = page_weights_map
            .get(p)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(9999999);
        let mut prefix = String::new();
        let parts: Vec<&str> = p.split('.').collect();
        for &head in &parts[..parts.len().saturating_sub(1)] {
            if prefix.is_empty() {
                prefix.push_str(head);
            } else {
                prefix.push('.');
                prefix.push_str(head);
            }
            is_dir_page.insert(prefix.clone());
            let cur_best_w = *dir_best_weight.get(&prefix).unwrap_or(&9999999);
            let cur_best_p = dir_best_page.get(&prefix).map(|s| s.as_str()).unwrap_or("");
            if w < cur_best_w || (w == cur_best_w && (cur_best_p.is_empty() || p.as_str() < cur_best_p))
            {
                dir_best_weight.insert(prefix.clone(), w);
                dir_best_page.insert(prefix.clone(), p.clone());
            }
        }
    }

    let sorted_pages_leaf_first = sort_page_hierarchy(
        &pages,
        '.',
        "leaf_first",
        None,
        &page_weights_map,
        "",
        Some(&dir_weights),
    );

    // 7. Build SEARCH_CONFIG JS
    let search_js = format_search_config_js(&search_block_types, &block_title_keys);

    // 8. Determine targets to render
    let build_targets: Vec<String> = if !cfg.target_page.is_empty() {
        if page_meta.contains_key(&cfg.target_page) {
            vec![cfg.target_page.clone()]
        } else {
            eprintln!("wiki:error: Page '{}' not found.", cfg.target_page);
            std::process::exit(1);
        }
    } else {
        pages.clone()
    };

    let render_targets: Vec<String> = build_targets
        .iter()
        .filter(|p| {
            force_rebuild
                || !cfg.target_page.is_empty()
                || page_meta.get(*p).map(|m| m.dirty).unwrap_or(true)
        })
        .cloned()
        .collect();

    // 9. Load templates & LaTeX cache only if rendering pages
    let latex_cache_file = cfg.cache_dir.join("latex.cache");
    let initial_latex = if !render_targets.is_empty() {
        load_latex_cache(&latex_cache_file)
    } else {
        HashMap::new()
    };

    let mut layout_cache: HashMap<PathBuf, String> = HashMap::new();
    let mut page_tmpl_paths: HashMap<String, PathBuf> = HashMap::new();

    for p in &render_targets {
        if let Some(tmpl_path) = resolve_template_path(p, &page_meta, &omni, &cfg.template_dir) {
            if !layout_cache.contains_key(&tmpl_path) {
                if let Ok(raw) = fs::read_to_string(&tmpl_path) {
                    layout_cache.insert(tmpl_path.clone(), raw);
                }
            }
            page_tmpl_paths.insert(p.clone(), tmpl_path);
        }
    }

    let mut ctx = WikiContext {
        cfg: cfg.clone(),
        omni,
        pages: pages.clone(),
        global_pages,
        nested_pages,
        sorted_pages_leaf_first,
        page_meta,
        dir_weights,
        is_dir_page,
        dir_best_page,
        refs,
        latex_cache: RwLock::new(initial_latex),
        layout_cache,
        wiki_menu_html: String::new(),
        wiki_featured_html: String::new(),
        search_js,
    };

    ctx.wiki_featured_html = build_featured_html(&ctx);
    ctx.wiki_menu_html = build_default_menu_html(&ctx, None);

    // 10. Pre-batch uncached LaTeX expressions in parallel before page rendering
    if !render_targets.is_empty() {
        prewarm_latex_parallel(&ctx, &render_targets);
    }

    // 11. Render dirty pages in parallel across worker threads
    let rendered_outputs: Mutex<Vec<PageRenderOutput>> = Mutex::new(Vec::new());
    if !render_targets.is_empty() {
        let jobs = ctx.cfg.jobs.max(1);
        let queue = Mutex::new(render_targets.iter());
        std::thread::scope(|s| {
            for _ in 0..jobs {
                let q_ref = &queue;
                let ctx_ref = &ctx;
                let tmpl_ref = &page_tmpl_paths;
                let out_ref = &rendered_outputs;
                s.spawn(move || loop {
                    let next_slug = {
                        let mut q = q_ref.lock().unwrap();
                        q.next().cloned()
                    };
                    let Some(slug) = next_slug else { break };
                    if let Some(tmpl_p) = tmpl_ref.get(&slug) {
                        if let Some(out) = render_single_page(ctx_ref, &slug, tmpl_p) {
                            write_page_idx_cache(&ctx_ref.cfg.cache_dir, &out);
                            out_ref.lock().unwrap().push(out);
                        }
                    }
                });
            }
        });

        save_latex_cache(&latex_cache_file, &ctx.latex_cache.read().unwrap());
    }

    // Save updated metadata cache
    save_meta_cache(&meta_cache_path, &ctx);

    // If single-page build, we're done!
    if !ctx.cfg.target_page.is_empty() {
        return;
    }

    // 12. Merge search index & defs.json in memory
    let mut merged_index: BTreeMap<String, String> = BTreeMap::new();
    let mut fresh_map: HashMap<String, PageRenderOutput> = rendered_outputs
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|o| (o.slug.clone(), o))
        .collect();

    for p in &ctx.pages {
        if let Some(fresh) = fresh_map.remove(p) {
            for (k, v) in fresh.search_entries {
                merged_index.entry(k).or_default().push_str(&v);
            }
            for (k, tk) in fresh.title_keys {
                block_title_keys.entry(k).or_insert(tk);
            }
        } else if let Some((entries, tkeys)) = read_page_idx_cache(&ctx.cfg.cache_dir, p) {
            for (k, v) in entries {
                merged_index.entry(k).or_default().push_str(&v);
            }
            for (k, tk) in tkeys {
                block_title_keys.entry(k).or_insert(tk);
            }
        }
    }

    write_search_index_json(&ctx.cfg.build_dir.join("index.json"), &merged_index);
    write_defs_index_json(
        &ctx.cfg.build_dir.join("defs.json"),
        merged_index.get("def").map(|s| s.as_str()).unwrap_or(""),
    );
    write_sitemap_xml(&ctx);
}

fn render_single_page(
    ctx: &WikiContext,
    pagename: &str,
    tmpl_path: &Path,
) -> Option<PageRenderOutput> {
    let pm = ctx.page_meta.get(pagename)?;
    let raw_layout = ctx.layout_cache.get(tmpl_path)?;
    let raw_content = fs::read_to_string(&pm.file_path).ok()?;

    let current_dir = pagename
        .rfind('.')
        .map(|i| &pagename[..i])
        .unwrap_or(".")
        .to_string();

    let mut state = RenderState {
        ctx,
        pagename: pagename.to_string(),
        file_path: pm.file_path.clone(),
        current_dir: current_dir.clone(),
        page_vars: pm.vars.clone(),
        toc_html: String::new(),
        page_table_count: 0,
        page_diagram_count: 0,
        search_index_map: BTreeMap::new(),
        list_html_map: HashMap::new(),
        page_title_keys: HashMap::new(),
    };

    let body_html = parse_page_body(&mut state, &raw_content);

    // Add page & tag entries to search_index_map
    let plain_text = strip_html_tags(&body_html);
    let enc_url = json_escape(&format!("/{}.html", pagename.replace('.', "/")));
    let enc_pagename = json_escape(pagename);
    let enc_category = json_escape(&current_dir);
    let file_tags = state
        .page_vars
        .get("page.tags")
        .or_else(|| state.page_vars.get("tags"))
        .cloned()
        .unwrap_or_else(|| pm.tags.clone());
    let enc_tags = json_escape(&file_tags);

    state
        .search_index_map
        .entry("page".to_string())
        .or_default()
        .push_str(&format!(
            "{{\"url\":\"{}\",\"pagename\":\"{}\",\"category\":\"{}\",\"content\":\"{}\",\"globals.tags\":\"{}\"}},\n",
            enc_url,
            enc_pagename,
            enc_category,
            json_escape(&plain_text),
            enc_tags
        ));

    if !file_tags.is_empty() {
        let tag_buf = state.search_index_map.entry("tag".to_string()).or_default();
        for t in file_tags.split([',', ' ']) {
            let t = t.trim();
            if !t.is_empty() {
                tag_buf.push_str(&format!(
                    "{{\"url\":\"{}\",\"tag\":\"{}\",\"pagename\":\"{}\"}},\n",
                    enc_url,
                    json_escape(t),
                    enc_pagename
                ));
            }
        }
    }

    let final_html = render_template(&state, raw_layout, &body_html);
    let out_path = ctx
        .cfg
        .build_dir
        .join(format!("{}.html", pagename.replace('.', "/")));
    if let Some(parent) = out_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&out_path, final_html);
    println!("wiki:success: Rendered -> {}", out_path.display());

    Some(PageRenderOutput {
        slug: pagename.to_string(),
        search_entries: state.search_index_map,
        title_keys: state.page_title_keys,
    })
}

fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(ch);
        }
    }
    out
}

fn prewarm_latex_parallel(ctx: &WikiContext, render_targets: &[String]) {
    let mut tasks: Vec<(bool, String)> = Vec::new();
    let mut seen: HashSet<(bool, String)> = HashSet::new();

    {
        let cache = ctx.latex_cache.read().unwrap();
        for slug in render_targets {
            let Some(pm) = ctx.page_meta.get(slug) else {
                continue;
            };
            let Ok(text) = fs::read_to_string(&pm.file_path) else {
                continue;
            };
            extract_latex_from_source(&text, &mut |inline, expr| {
                let key = if inline {
                    format!("inline:{}", expr)
                } else {
                    format!("block:{}", expr)
                };
                if !cache.contains_key(&key) && seen.insert((inline, expr.to_string())) {
                    tasks.push((inline, expr.to_string()));
                }
            });
        }
    }

    if tasks.is_empty() {
        return;
    }

    let jobs = ctx.cfg.jobs.max(1);
    let q = Mutex::new(tasks.into_iter());
    std::thread::scope(|s| {
        for _ in 0..jobs {
            let q_ref = &q;
            let ctx_ref = ctx;
            s.spawn(move || loop {
                let item = q_ref.lock().unwrap().next();
                let Some((inline, expr)) = item else { break };
                if let Some(svg) = render_latex_expr(&ctx_ref.cfg.wiki_dir, &expr, inline) {
                    let key = if inline {
                        format!("inline:{}", expr)
                    } else {
                        format!("block:{}", expr)
                    };
                    ctx_ref.latex_cache.write().unwrap().insert(key, svg);
                }
            });
        }
    });
}

fn extract_latex_from_source<F: FnMut(bool, &str)>(text: &str, cb: &mut F) {
    let mut in_tex = 0u8;
    let mut tex_buf = String::new();

    for raw_line in text.lines() {
        let mut c_line = raw_line.trim_end_matches('\r').trim_start();
        if let Some(after) = c_line.strip_prefix("::") {
            c_line = after.trim_start();
        }
        let r_line = c_line.trim_end();

        if in_tex == 1 {
            if r_line.starts_with("{{")
                && r_line.ends_with("}}")
                && r_line[2..r_line.len() - 2].trim() == "block.end"
            {
                if !tex_buf.is_empty() {
                    cb(false, &tex_buf);
                }
                in_tex = 0;
                tex_buf.clear();
            } else {
                if !tex_buf.is_empty() {
                    tex_buf.push('\n');
                }
                tex_buf.push_str(c_line);
            }
            continue;
        } else if in_tex == 2 {
            if r_line == "$$" {
                if !tex_buf.is_empty() {
                    cb(false, &tex_buf);
                }
                in_tex = 0;
                tex_buf.clear();
            } else {
                if !tex_buf.is_empty() {
                    tex_buf.push('\n');
                }
                tex_buf.push_str(c_line);
            }
            continue;
        }

        if r_line.starts_with("{{")
            && r_line.ends_with("}}")
            && r_line[2..r_line.len() - 2].trim() == "block.tex"
        {
            in_tex = 1;
            tex_buf.clear();
            continue;
        }
        if r_line == "$$" {
            in_tex = 2;
            tex_buf.clear();
            continue;
        }
        if r_line.starts_with("$$") && r_line.ends_with("$$") && r_line.len() > 4 {
            let b_m = &r_line[2..r_line.len() - 2];
            if !b_m.is_empty() {
                cb(false, b_m);
            }
            continue;
        }

        if raw_line.contains('$') {
            let mut s = raw_line.replace("$$", "");
            while let Some(i1) = s.find('`') {
                let rest = &s[i1 + 1..];
                if let Some(i2) = rest.find('`') {
                    s = format!("{}{}", &s[..i1], &rest[i2 + 1..]);
                } else {
                    break;
                }
            }
            let mut rem = s.as_str();
            while let Some(i1) = rem.find('$') {
                let rest = &rem[i1 + 1..];
                if let Some(i2) = rest.find('$') {
                    let m = &rest[..i2];
                    if !m.is_empty() {
                        cb(true, m);
                    }
                    rem = &rest[i2 + 1..];
                } else {
                    break;
                }
            }
        }
    }
}

fn write_search_index_json(path: &Path, map: &BTreeMap<String, String>) {
    let mut out = String::from("{");
    let mut first = true;
    for (k, v) in map {
        let trimmed = v.trim_end_matches('\n').trim_end_matches(',');
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&format!("\"{}\": [ {} ]", k, trimmed));
    }
    out.push('}');
    let _ = fs::write(path, out);
}

fn write_defs_index_json(path: &Path, defs_data: &str) {
    if defs_data.trim().is_empty() {
        let _ = fs::write(path, "{\n}\n");
        println!("wiki:success: Generated defs index -> {}", path.display());
        return;
    }

    let mut keys_order: Vec<String> = Vec::new();
    let mut defs_urls: HashMap<String, String> = HashMap::new();
    let mut defs_deps: HashMap<String, Vec<String>> = HashMap::new();
    let mut defs_rdeps: HashMap<String, Vec<String>> = HashMap::new();
    let mut seen_deps: HashSet<(String, String)> = HashSet::new();
    let mut seen_rdeps: HashSet<(String, String)> = HashSet::new();

    for line in defs_data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let url = extract_json_str(line, "url");
        let context = extract_json_str(line, "__context__");
        let term = extract_json_str(line, "term");
        let id = extract_json_str(line, "__id__");
        let related = extract_json_str(line, "related");

        let raw_id = if !id.is_empty() { id } else { term };
        if raw_id.is_empty() {
            continue;
        }
        let ref_key = if !context.is_empty() && !raw_id.contains('.') {
            slugify(&format!("{}.{}", context, raw_id))
        } else {
            slugify(&raw_id)
        };

        if !defs_urls.contains_key(&ref_key) {
            defs_urls.insert(ref_key.clone(), url);
            keys_order.push(ref_key.clone());
        }

        if !related.is_empty() {
            for item in related.split(',') {
                let r_raw = item.trim().strip_prefix("def.").unwrap_or(item.trim());
                if !r_raw.is_empty() {
                    let r = slugify(r_raw);
                    if !r.is_empty() {
                        if seen_deps.insert((ref_key.clone(), r.clone())) {
                            defs_deps
                                .entry(ref_key.clone())
                                .or_default()
                                .push(format!("\"{}\"", r));
                        }
                        if seen_rdeps.insert((r.clone(), ref_key.clone())) {
                            defs_rdeps
                                .entry(r)
                                .or_default()
                                .push(format!("\"{}\"", ref_key));
                        }
                    }
                }
            }
        }
    }

    let mut out = String::from("{");
    for (i, k) in keys_order.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let u = defs_urls.get(k).map(|s| s.as_str()).unwrap_or("");
        let deps = defs_deps.get(k).map(|v| v.join(",")).unwrap_or_default();
        let rdeps = defs_rdeps.get(k).map(|v| v.join(",")).unwrap_or_default();
        out.push_str(&format!(
            "\n  \"{}\": {{ \"url\": \"{}\", \"deps\": [ {} ], \"rdeps\": [ {} ] }}",
            k, u, deps, rdeps
        ));
    }
    out.push_str("\n}\n");
    let _ = fs::write(path, out);
    println!("wiki:success: Generated defs index -> {}", path.display());
}

fn extract_json_str(line: &str, key: &str) -> String {
    let pat = format!("\"{}\":\"", key);
    let Some(idx) = line.find(&pat) else {
        return String::new();
    };
    let rest = &line[idx + pat.len()..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(esc) = chars.next() {
                out.push(esc);
            }
        } else if c == '"' {
            break;
        } else {
            out.push(c);
        }
    }
    out
}

fn write_sitemap_xml(ctx: &WikiContext) {
    if ctx.cfg.domain.is_empty() {
        return;
    }
    let mut domain = ctx.cfg.domain.trim_end_matches('/').to_string();
    if !domain.starts_with("http://") && !domain.starts_with("https://") {
        domain = format!("https://{}", domain);
    }

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for p in &ctx.pages {
        let path = format!("{}.html", p.replace('.', "/"));
        let loc = if path == "index.html" {
            format!("{}/", domain)
        } else {
            format!("{}/{}", domain, path)
        };
        xml.push_str(&format!(
            "  <url>\n    <loc>{}</loc>\n    <lastmod>{}</lastmod>\n  </url>\n",
            loc, ctx.cfg.build_date_iso
        ));
    }
    xml.push_str("</urlset>\n");
    let out_p = ctx.cfg.build_dir.join("sitemap.xml");
    let _ = fs::write(&out_p, xml);
    println!("wiki:success: Generated sitemap -> {}", out_p.display());
}

fn resolve_template_path(
    pagename: &str,
    page_meta: &HashMap<String, PageMeta>,
    omni: &model::OmniEnv,
    template_dir: &Path,
) -> Option<PathBuf> {
    let mut p_tmpl = page_meta
        .get(pagename)
        .map(|m| m.template.clone())
        .unwrap_or_default();

    if p_tmpl.is_empty() {
        let cur_dir = pagename.rfind('.').map(|i| &pagename[..i]).unwrap_or(".");
        let idx_slug = if cur_dir == "." {
            "index".to_string()
        } else {
            format!("{}.index", cur_dir)
        };
        if pagename != idx_slug {
            if let Some(m) = page_meta.get(&idx_slug) {
                p_tmpl = m.template.clone();
            }
        }
    }

    if let Some(mapped) = omni.templates.get(&p_tmpl) {
        let pb = PathBuf::from(mapped);
        if pb.is_dir() {
            return Some(pb.join("layout.html"));
        } else if pb.is_file() {
            return Some(pb);
        }
    }

    let cand = if p_tmpl.is_empty() {
        template_dir.to_path_buf()
    } else {
        let with_ext = if p_tmpl.contains('.') {
            p_tmpl
        } else {
            format!("{}.html", p_tmpl)
        };
        let p = PathBuf::from(&with_ext);
        if p.is_absolute() {
            p
        } else {
            template_dir.join(with_ext)
        }
    };

    if cand.is_dir() {
        let l = cand.join("layout.html");
        if l.is_file() {
            return Some(l);
        }
    } else if cand.is_file() {
        return Some(cand);
    }
    eprintln!(
        "wiki:error: Template file missing for page {} at {}",
        pagename,
        cand.display()
    );
    None
}

fn format_search_config_js(
    types: &BTreeSet<String>,
    title_keys: &HashMap<String, String>,
) -> String {
    let mut s = String::from("const SEARCH_CONFIG = [");
    let mut first = true;
    for sk in types {
        if !first {
            s.push(',');
        }
        first = false;
        let mut chars = sk.chars();
        let fallback = match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        };
        let tkey = match sk.as_str() {
            "page" | "tag" => "pagename",
            "table" => "caption",
            other => title_keys.get(other).map(|v| v.as_str()).unwrap_or(other),
        };
        s.push_str(&format!(
            " {{ id: '{}', others: ['{}s'], titleKey: '{}', fallback: '{}' }}",
            sk, sk, tkey, fallback
        ));
    }
    s.push_str(" ];");
    s
}

fn collect_wiki_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_wiki_files(&p, out);
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            if ext == "wiki" || ext == "md" {
                out.push(p);
            }
        }
    }
}

fn derive_slug_and_dir_weights(
    rel: &Path,
    dir_weights: &mut HashMap<String, String>,
) -> (String, String) {
    let rel_str = rel.to_string_lossy();
    let dot = rel_str.rfind('.').unwrap_or(rel_str.len());
    let no_ext = &rel_str[..dot];
    let parts: Vec<&str> = no_ext.split('/').collect();

    let mut clean_slug = String::new();
    let mut implicit_weight = String::new();

    for (i, &raw_part) in parts.iter().enumerate() {
        let (part_w, part) = if let Some((w, r)) = split_num_prefix(raw_part) {
            (w.to_string(), r)
        } else if !raw_part.is_empty() && raw_part.bytes().all(|b| b.is_ascii_digit()) {
            (raw_part.to_string(), raw_part)
        } else {
            (String::new(), raw_part)
        };

        if clean_slug.is_empty() {
            clean_slug.push_str(part);
        } else {
            clean_slug.push('.');
            clean_slug.push_str(part);
        }

        if i == parts.len() - 1 {
            implicit_weight = part_w;
        } else if !part_w.is_empty() && !dir_weights.contains_key(&clean_slug) {
            dir_weights.insert(clean_slug.clone(), part_w);
        }
    }
    (clean_slug, implicit_weight)
}

fn file_mtime_size(p: &Path) -> (u64, u64) {
    if let Ok(md) = fs::metadata(p) {
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        (mtime, md.len())
    } else {
        (0, 0)
    }
}

fn compute_tree_fingerprint(paths: &[PathBuf]) -> String {
    let mut acc: Vec<String> = Vec::new();
    fn walk(base: &Path, p: &Path, acc: &mut Vec<String>) {
        if p.is_file() {
            let h = fs::read(p).map(|b| fnv1a_hash(&b)).unwrap_or(0);
            let rel = if base == p {
                p.file_name().map(Path::new).unwrap_or(p)
            } else {
                p.strip_prefix(base).unwrap_or(p)
            };
            acc.push(format!("{:016x}_{}", h, rel.to_string_lossy()));
        } else if p.is_dir() {
            if let Ok(entries) = fs::read_dir(p) {
                for e in entries.flatten() {
                    walk(base, &e.path(), acc);
                }
            }
        }
    }
    for p in paths {
        walk(p, p, &mut acc);
    }
    acc.sort();
    format!("{:016x}", fnv1a_hash(acc.join("\n").as_bytes()))
}

fn compute_dir_stat_fingerprint(dir: &Path) -> String {
    let mut acc: Vec<String> = Vec::new();
    fn walk(base: &Path, p: &Path, acc: &mut Vec<String>) {
        if p.is_file() {
            let (m, s) = file_mtime_size(p);
            let rel = p.strip_prefix(base).unwrap_or(p);
            acc.push(format!("{}_{}_{}", m, s, rel.to_string_lossy()));
        } else if p.is_dir() {
            if let Ok(entries) = fs::read_dir(p) {
                for e in entries.flatten() {
                    walk(base, &e.path(), acc);
                }
            }
        }
    }
    walk(dir, dir, &mut acc);
    acc.sort();
    format!("{:016x}", fnv1a_hash(acc.join("\n").as_bytes()))
}

fn sync_static_if_needed(cfg: &Config, force: bool) {
    if !cfg.static_dir.is_dir() {
        return;
    }
    let out_static = cfg.build_dir.join("_static");
    let hash_file = cfg.cache_dir.join("rust_static.hash");
    let fp = compute_dir_stat_fingerprint(&cfg.static_dir);
    let old_fp = fs::read_to_string(&hash_file).unwrap_or_default();
    if !force && fp == old_fp.trim() && out_static.is_dir() {
        return;
    }
    let _ = copy_dir_recursive(&cfg.static_dir, &out_static);
    let _ = fs::write(&hash_file, format!("{}\n", fp));
}

fn sync_assets_if_needed(cfg: &Config, force: bool) {
    let out_assets = cfg.build_dir.join("_assets");
    let out_existed = out_assets.is_dir();
    let _ = fs::create_dir_all(&out_assets);
    if !cfg.assets_dir.is_dir() {
        return;
    }
    let hash_file = cfg.cache_dir.join("rust_assets.hash");
    let fp = compute_dir_stat_fingerprint(&cfg.assets_dir);
    let old_fp = fs::read_to_string(&hash_file).unwrap_or_default();
    if !force && out_existed && fp == old_fp.trim() {
        return;
    }

    let cwebp_bin = find_cwebp_bin(&cfg.wiki_dir);
    let mut images: Vec<PathBuf> = Vec::new();
    collect_convertible_images(&cfg.assets_dir, &mut images);

    if !images.is_empty() {
        if let Some(ref bin) = cwebp_bin {
            let jobs = cfg.jobs.max(1);
            let q = Mutex::new(images.into_iter());
            std::thread::scope(|s| {
                for _ in 0..jobs {
                    let q_ref = &q;
                    s.spawn(move || loop {
                        let next_img = q_ref.lock().unwrap().next();
                        let Some(img) = next_img else { break };
                        let webp = img.with_extension("webp");
                        if !webp.exists() {
                            let _ = Command::new(bin)
                                .args(["-q", "80"])
                                .arg(&img)
                                .arg("-o")
                                .arg(&webp)
                                .output();
                        }
                    });
                }
            });
        }
    }

    let _ = copy_dir_recursive(&cfg.assets_dir, &out_assets);
    let fp_after = compute_dir_stat_fingerprint(&cfg.assets_dir);
    let _ = fs::write(&hash_file, format!("{}\n", fp_after));
}


fn collect_convertible_images(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_convertible_images(&p, out);
        } else if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
            if matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg")
                && !p.with_extension("webp").exists()
            {
                out.push(p);
            }
        }
    }
}

fn find_cwebp_bin(wiki_dir: &Path) -> Option<PathBuf> {
    for cand in [wiki_dir.join("bin/cwep"), wiki_dir.join("bin/cwebp")] {
        if cand.is_file() {
            return Some(cand);
        }
    }
    if Command::new("cwebp").arg("-version").output().is_ok() {
        return Some(PathBuf::from("cwebp"));
    }
    None
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let sp = entry.path();
        let dp = dst.join(entry.file_name());
        if sp.is_dir() {
            copy_dir_recursive(&sp, &dp)?;
        } else {
            let need_copy = if let (Ok(sm), Ok(dm)) = (fs::metadata(&sp), fs::metadata(&dp)) {
                sm.len() != dm.len() || sm.modified().ok() > dm.modified().ok()
            } else {
                true
            };
            if need_copy {
                fs::copy(&sp, &dp)?;
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct CachedMetaEntry {
    mtime: u64,
    size: u64,
    hash: u64,
    title: String,
    weight: String,
    template: String,
    tags: String,
    thumb: String,
    related: String,
    author: String,
    partials: String,
    vars: HashMap<String, String>,
    block_types: HashSet<String>,
    block_title_keys: HashMap<String, String>,
    refs: Vec<(String, String, String)>,
}

fn save_meta_cache(path: &Path, ctx: &WikiContext) {
    let mut out = String::new();
    for p in &ctx.pages {
        let Some(pm) = ctx.page_meta.get(p) else {
            continue;
        };
        let p_str = pm.file_path.to_string_lossy();
        let btypes: Vec<&str> = pm.block_types.iter().map(|s| s.as_str()).collect();
        let bkeys: Vec<String> = pm
            .block_title_keys
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        let vars_enc: Vec<String> = pm
            .vars
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}\x1D{}",
                    k,
                    v.replace(['\n', '\r', '\x1F', '\x1E', '\x1D'], " ")
                )
            })
            .collect();

        let prefix_target = format!("{}:", p);
        let mut page_refs: Vec<String> = Vec::new();
        for (rk, rv) in &ctx.refs {
            if !rk.contains("::") {
                continue;
            }
            if rk.matches("::").count() == 1 && rv.starts_with(&prefix_target) {
                if let Some((btype, rkey)) = rk.split_once("::") {
                    let anc = &rv[prefix_target.len()..];
                    page_refs.push(format!("{}\x1D{}\x1D{}", btype, rkey, anc));
                }
            }
        }

        out.push_str(&format!(
            "{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\x1F{}\n",
            p_str,
            pm.mtime,
            pm.size,
            pm.hash,
            pm.title,
            pm.weight,
            pm.template,
            pm.tags,
            pm.thumb,
            pm.related,
            pm.author,
            pm.partials,
            btypes.join(","),
            bkeys.join(","),
            vars_enc.join("\x1E"),
            page_refs.join("\x1E")
        ));
    }
    let _ = fs::write(path, out);
}

fn load_meta_cache(path: &Path) -> HashMap<String, CachedMetaEntry> {
    let mut map = HashMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        return map;
    };
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\x1F').collect();
        if fields.len() < 16 {
            continue;
        }
        let mut entry = CachedMetaEntry {
            mtime: fields[1].parse().unwrap_or(0),
            size: fields[2].parse().unwrap_or(0),
            hash: fields[3].parse().unwrap_or(0),
            title: fields[4].to_string(),
            weight: fields[5].to_string(),
            template: fields[6].to_string(),
            tags: fields[7].to_string(),
            thumb: fields[8].to_string(),
            related: fields[9].to_string(),
            author: fields[10].to_string(),
            partials: fields[11].to_string(),
            ..Default::default()
        };
        for bt in fields[12].split(',').filter(|s| !s.is_empty()) {
            entry.block_types.insert(bt.to_string());
        }
        for bk in fields[13].split(',').filter(|s| !s.is_empty()) {
            if let Some((k, v)) = bk.split_once('=') {
                entry.block_title_keys.insert(k.to_string(), v.to_string());
            }
        }
        for kv in fields[14].split('\x1E').filter(|s| !s.is_empty()) {
            if let Some((k, v)) = kv.split_once('\x1D') {
                entry.vars.insert(k.to_string(), v.to_string());
            }
        }
        for rf in fields[15].split('\x1E').filter(|s| !s.is_empty()) {
            let mut sp = rf.splitn(3, '\x1D');
            if let (Some(bt), Some(rk), Some(anc)) = (sp.next(), sp.next(), sp.next()) {
                entry
                    .refs
                    .push((bt.to_string(), rk.to_string(), anc.to_string()));
            }
        }
        map.insert(fields[0].to_string(), entry);
    }
    map
}

fn write_page_idx_cache(cache_dir: &Path, out: &PageRenderOutput) {
    let p = cache_dir.join(format!("{}_rust.idx", out.slug));
    let mut buf = String::new();
    for (k, tk) in &out.title_keys {
        buf.push_str(&format!("TK\x1F{}\x1F{}\n", k, tk));
    }
    for (k, chunk) in &out.search_entries {
        let escaped = chunk.replace('\n', "\x01");
        buf.push_str(&format!("IDX\x1F{}\x1F{}\n", k, escaped));
    }
    let _ = fs::write(p, buf);
}

fn read_page_idx_cache(
    cache_dir: &Path,
    slug: &str,
) -> Option<(BTreeMap<String, String>, HashMap<String, String>)> {
    let p = cache_dir.join(format!("{}_rust.idx", slug));
    let text = fs::read_to_string(p).ok()?;
    let mut entries = BTreeMap::new();
    let mut tkeys = HashMap::new();
    for line in text.lines() {
        let mut sp = line.splitn(3, '\x1F');
        match (sp.next(), sp.next(), sp.next()) {
            (Some("TK"), Some(k), Some(tk)) => {
                tkeys.insert(k.to_string(), tk.to_string());
            }
            (Some("IDX"), Some(k), Some(c)) => {
                entries.insert(k.to_string(), c.replace('\x01', "\n"));
            }
            _ => {}
        }
    }
    Some((entries, tkeys))
}

fn load_latex_cache(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        return map;
    };
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('\t') {
            map.insert(k.to_string(), v.replace('\x01', "\n"));
        }
    }
    map
}

fn save_latex_cache(path: &Path, map: &HashMap<String, String>) {
    if map.is_empty() {
        return;
    }
    let mut out = String::new();
    for (k, v) in map {
        out.push_str(k);
        out.push('\t');
        out.push_str(&v.replace('\n', "\x01"));
        out.push('\n');
    }
    let _ = fs::write(path, out);
}

fn find_arg_val(flag: &str) -> Option<String> {
    let args: Vec<String> = env::args().collect();
    for i in 1..args.len() {
        if args[i] == flag && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}

fn parse_cli() -> Config {
    let args: Vec<String> = env::args().collect();
    let mut wiki_name = String::new();
    let mut wiki_dir = PathBuf::from(".");
    let mut source_dir = PathBuf::from("content");
    let mut build_dir = PathBuf::from("dist");
    let mut template_dir = PathBuf::from("templates");
    let mut static_dir = PathBuf::from("static");
    let mut assets_dir = PathBuf::from("assets");
    let mut cache_dir = PathBuf::new();
    let mut domain = String::new();
    let mut target_page = String::new();
    let mut jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let mut force_rebuild = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
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
            "--dist" if i + 1 < args.len() => {
                build_dir = PathBuf::from(&args[i + 1]);
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
            "--domain" if i + 1 < args.len() => {
                domain = args[i + 1].clone();
                i += 2;
            }
            "--page" if i + 1 < args.len() => {
                target_page = args[i + 1].clone();
                i += 2;
            }
            "--jobs" if i + 1 < args.len() => {
                jobs = args[i + 1].parse().unwrap_or(4).max(1);
                i += 2;
            }
            "--rebuild" => {
                force_rebuild = true;
                i += 1;
            }
            "--config" if i + 1 < args.len() => {
                i += 2;
            }
            _ => i += 1,
        }
    }

    if cache_dir.as_os_str().is_empty() {
        cache_dir = build_dir.join(".cache");
    }

    let (build_year, build_now, build_date_iso) = current_datetime_strings();
    let wiki_env = env::var("ENV").unwrap_or_default();

    Config {
        wiki_name,
        wiki_env,
        wiki_dir,
        source_dir,
        build_dir,
        template_dir,
        static_dir,
        assets_dir,
        cache_dir,
        domain,
        target_page,
        jobs,
        force_rebuild,
        build_year,
        build_now,
        build_date_iso,
    }
}

fn current_datetime_strings() -> (String, String, String) {
    if let Ok(out) = Command::new("date")
        .arg("+%Y|%Y/%m/%d at %H:%M|%Y-%m-%d")
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let parts: Vec<&str> = s.trim().split('|').collect();
            if parts.len() == 3 {
                return (
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].to_string(),
                );
            }
        }
    }
    (
        "2026".to_string(),
        "2026/01/01 at 00:00".to_string(),
        "2026-01-01".to_string(),
    )
}
