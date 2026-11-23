use crate::model::{glob_match, parse_kv_args, RenderState, WikiContext};
use crate::parser::listing_pages;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::process::Command;

pub fn build_featured_html(ctx: &WikiContext) -> String {
    let featured_list: Vec<String> = if !ctx.omni.featured.is_empty() {
        ctx.omni.featured.clone()
    } else {
        ctx.global_pages.clone()
    };

    let mut seen: HashSet<String> = HashSet::new();
    let mut resolved: Vec<(u64, String, String, String)> = Vec::new(); // (weight, fpage, url, title)

    for fpage in featured_list {
        if !seen.insert(fpage.clone()) {
            continue;
        }
        let (mut w_str, mut t, url) = if let Some(pm) = ctx.page_meta.get(&fpage) {
            (
                pm.weight.clone(),
                pm.title.clone(),
                format!("/{}.html", fpage.replace('.', "/")),
            )
        } else if let Some(pm) = ctx.page_meta.get(&format!("{}.index", fpage)) {
            (
                pm.weight.clone(),
                pm.title.clone(),
                format!("/{}/index.html", fpage.replace('.', "/")),
            )
        } else if let Some(best_p) = ctx.dir_best_page.get(&fpage) {
            let pm_opt = ctx.page_meta.get(best_p);
            (
                pm_opt
                    .map(|m| m.weight.clone())
                    .unwrap_or_else(|| "9999".to_string()),
                pm_opt.map(|m| m.title.clone()).unwrap_or_default(),
                format!("/{}.html", best_p.replace('.', "/")),
            )
        } else {
            (
                "9999".to_string(),
                capitalize_first(fpage.rsplit('.').next().unwrap_or(&fpage)),
                format!("/{}.html", fpage.replace('.', "/")),
            )
        };

        if t.is_empty() {
            t = capitalize_first(fpage.rsplit('.').next().unwrap_or(&fpage));
        }
        if w_str.is_empty() || !w_str.bytes().all(|b| b.is_ascii_digit()) {
            w_str = "9999".to_string();
        }
        let w_num = w_str.parse::<u64>().unwrap_or(9999);
        resolved.push((w_num, fpage, url, t));
    }

    resolved.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut html = String::new();
    for (_, _, url, title) in resolved {
        html.push_str(&format!("<a href=\"{}\" class=\"menu-item\">{}</a>\n", url, title));
    }
    html
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

pub fn build_default_menu_html(ctx: &WikiContext, current_page: Option<&str>) -> String {
    let mut menu_parts: Vec<String> = vec!["<ul class=\"menu-ul\">".to_string()];
    let mut current_path: Vec<String> = Vec::new();

    for entry in &ctx.sorted_pages_leaf_first {
        if entry.is_empty() {
            continue;
        }
        let parts: Vec<&str> = entry.split('.').collect();
        let page_name = parts[parts.len() - 1];
        let dir_parts = &parts[..parts.len() - 1];

        let mut common_len = 0usize;
        while common_len < current_path.len()
            && common_len < dir_parts.len()
            && current_path[common_len] == dir_parts[common_len]
        {
            common_len += 1;
        }
        for _ in common_len..current_path.len() {
            menu_parts.push("</ul></details></li>".to_string());
        }
        for (i, &dir) in dir_parts.iter().enumerate().skip(common_len) {
            let css_class = if i > 0 {
                "menu-details sub-dir"
            } else {
                "menu-details"
            };
            menu_parts.push(format!(
                "<li class=\"{}\"><details open><summary><span class=\"toggle-icon\"></span>{}/</summary><ul class=\"menu-ul-inner\">",
                css_class, dir
            ));
        }
        let page_url = format!("{}.html", entry.replace('.', "/"));
        let disp_name = ctx
            .page_meta
            .get(entry)
            .and_then(|m| if m.title.is_empty() { None } else { Some(m.title.as_str()) })
            .unwrap_or(page_name);
        let current_attr = if current_page == Some(entry.as_str()) {
            " aria-current=\"page\""
        } else {
            ""
        };
        menu_parts.push(format!(
            "<li class=\"menu-li\"><a href=\"/{}\"{}>{}</a></li>",
            page_url, current_attr, disp_name
        ));
        current_path = dir_parts.iter().map(|s| s.to_string()).collect();
    }

    for _ in 0..current_path.len() {
        menu_parts.push("</ul></details></li>".to_string());
    }
    menu_parts.push("</ul>".to_string());
    format!("{}\n", menu_parts.join("\n"))
}

pub fn partial_menu(ctx: &WikiContext, pagename: &str, args: &str) -> String {
    if args.trim().is_empty() {
        return build_default_menu_html(ctx, Some(pagename));
    }

    let mut m_root = String::new();
    let mut m_depth = 999usize;
    let mut m_type = String::new();
    let mut m_root_label = String::new();
    let mut m_page_label = "title".to_string();
    let mut m_exclude = String::new();
    let mut m_closed: i32 = -1;
    let mut m_open_current = "0".to_string();
    let mut m_collapsible = false;

    for (k, v) in parse_kv_args(args) {
        match k.as_str() {
            "root" => m_root = v,
            "depth" => m_depth = v.parse::<usize>().unwrap_or(999),
            "type" => m_type = v,
            "root_label" => m_root_label = v,
            "page_label" => m_page_label = v,
            "exclude" => m_exclude = v,
            "closed" => m_closed = v.parse::<i32>().unwrap_or(-1),
            "open_current" => m_open_current = v,
            "collapsible" => m_collapsible = v.eq_ignore_ascii_case("true") || v == "1",
            _ => {}
        }
    }

    let orig_root = m_root.clone();
    let trimmed_root = m_root.trim_matches('/');
    let mut prefix = trimmed_root.replace('/', ".");
    if !prefix.is_empty() {
        prefix.push('.');
    }

    let mut menu_parts: Vec<String> = Vec::new();
    let global_open = if m_closed != -1 || m_open_current != "0" {
        " open"
    } else {
        ""
    };
    if m_collapsible {
        menu_parts.push(format!(
            "<details class=\"menu-global-details\"{}><summary><span class=\"toggle-icon\"></span><span class=\"menu-global-text\"></span></summary>",
            global_open
        ));
    }
    menu_parts.push("<ul class=\"menu-ul\">".to_string());

    if !trimmed_root.is_empty() || orig_root == "/" {
        let root_node = prefix.trim_end_matches('.');
        let mut root_disp = if !m_root_label.is_empty() {
            m_root_label.clone()
        } else {
            let mut d = if !root_node.is_empty() {
                ctx.page_meta
                    .get(root_node)
                    .map(|m| m.title.clone())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            if d.is_empty() {
                d = trimmed_root.to_string();
            }
            if d.is_empty() {
                d = "/".to_string();
            }
            d
        };
        if !root_disp.ends_with('/') {
            root_disp.push('/');
        }
        let mut root_open = if m_closed >= 0 && 0 >= m_closed {
            ""
        } else {
            " open"
        };
        if m_open_current == "1" && !pagename.is_empty() && pagename.starts_with(&prefix) {
            root_open = " open";
        }
        menu_parts.push(format!(
            "<li class=\"menu-details\"><details{}><summary><span class=\"toggle-icon\"></span>{}</summary>",
            root_open, root_disp
        ));
        menu_parts.push("<ul class=\"menu-ul-inner\">".to_string());
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut current_path: Vec<String> = Vec::new();

    for p in &ctx.sorted_pages_leaf_first {
        if !prefix.is_empty() && !p.starts_with(&prefix) {
            continue;
        }
        let rem = &p[prefix.len()..];
        if rem.is_empty() {
            continue;
        }
        if !m_exclude.is_empty() && glob_match(&m_exclude, p) {
            continue;
        }

        let parts: Vec<&str> = rem.split('.').collect();
        let p_depth = parts.len();
        let mut is_dir = false;
        let mut target_depth = p_depth;
        if p_depth > m_depth {
            target_depth = m_depth;
            is_dir = true;
        } else if ctx.is_dir_page.contains(p) {
            is_dir = true;
        }

        if m_type == "dir" && !is_dir {
            continue;
        }
        if m_type == "file" && is_dir {
            continue;
        }
        if target_depth == 0 {
            continue;
        }

        let node_rem = if target_depth == p_depth {
            rem.to_string()
        } else {
            parts[..target_depth].join(".")
        };
        let node_full = format!("{}{}", prefix, node_rem);

        if seen.insert(node_full.clone()) {
            let mut disp_name = if m_page_label == "weight" {
                ctx.page_meta
                    .get(&node_full)
                    .map(|m| m.weight.clone())
                    .unwrap_or_else(|| "9999999".to_string())
            } else if m_page_label == "name" {
                parts[target_depth - 1].to_string()
            } else {
                ctx.page_meta
                    .get(&node_full)
                    .and_then(|m| {
                        if m.title.is_empty() {
                            None
                        } else {
                            Some(m.title.clone())
                        }
                    })
                    .unwrap_or_else(|| parts[target_depth - 1].to_string())
            };

            let page_url = if is_dir {
                if !disp_name.ends_with('/') {
                    disp_name.push('/');
                }
                if let Some(best_p) = ctx.dir_best_page.get(&node_full) {
                    format!("{}.html", best_p.replace('.', "/"))
                } else {
                    format!("{}/index.html", node_full.replace('.', "/"))
                }
            } else {
                format!("{}.html", node_full.replace('.', "/"))
            };

            let dir_parts = &parts[..target_depth - 1];
            let mut common_len = 0usize;
            while common_len < current_path.len()
                && common_len < dir_parts.len()
                && current_path[common_len] == dir_parts[common_len]
            {
                common_len += 1;
            }
            for _ in common_len..current_path.len() {
                menu_parts.push("</ul></details></li>".to_string());
            }

            let mut dir_full: Vec<String> = Vec::with_capacity(dir_parts.len());
            if m_open_current == "1" {
                let mut acc = String::new();
                for &dp in dir_parts {
                    if !acc.is_empty() {
                        acc.push('.');
                    }
                    acc.push_str(dp);
                    dir_full.push(acc.clone());
                }
            }

            for (k, &dir) in dir_parts.iter().enumerate().skip(common_len) {
                let css_class = if k > 0 {
                    "menu-details sub-dir"
                } else {
                    "menu-details"
                };
                let current_depth = if !trimmed_root.is_empty() || orig_root == "/" {
                    (k + 1) as i32
                } else {
                    k as i32
                };
                let mut dir_open = if m_closed >= 0 && current_depth >= m_closed {
                    ""
                } else {
                    " open"
                };
                if m_open_current == "1"
                    && !pagename.is_empty()
                    && pagename.starts_with(&format!("{}{}.", prefix, dir_full[k]))
                {
                    dir_open = " open";
                }
                menu_parts.push(format!(
                    "<li class=\"{}\"><details{}><summary><span class=\"toggle-icon\"></span>{}/</summary><ul class=\"menu-ul-inner\">",
                    css_class, dir_open, dir
                ));
            }

            let current_attr = if !pagename.is_empty() && node_full == pagename {
                " aria-current=\"page\""
            } else {
                ""
            };
            menu_parts.push(format!(
                "<li class=\"menu-li\"><a href=\"/{}\"{}>{}</a></li>",
                page_url, current_attr, disp_name
            ));
            current_path = dir_parts.iter().map(|s| s.to_string()).collect();
        }
    }

    for _ in 0..current_path.len() {
        menu_parts.push("</ul></details></li>".to_string());
    }
    if !trimmed_root.is_empty() || orig_root == "/" {
        menu_parts.push("</ul></details></li>".to_string());
    }
    menu_parts.push("</ul>".to_string());
    if m_collapsible {
        menu_parts.push("</details>".to_string());
    }
    format!("{}\n", menu_parts.join("\n"))
}

pub fn partial_breadcrumb(ctx: &WikiContext, pagename: &str, args: &str) -> String {
    let mut b_root_label = "Home".to_string();
    let mut b_page_label = "title".to_string();
    let mut b_sep = "/".to_string();

    for (k, v) in parse_kv_args(args) {
        match k.as_str() {
            "root_label" => b_root_label = v,
            "page_label" | "label" => b_page_label = v,
            "sep" => b_sep = v,
            _ => {}
        }
    }

    let mut out = format!(
        "<breadcrumb><a href=\"/index.html\" class=\"breadcrumb-0\">{}</a>",
        b_root_label
    );
    let b_parts: Vec<&str> = pagename.split('.').collect();
    let mut cum_slug = String::new();

    for (i, &part) in b_parts.iter().enumerate() {
        if cum_slug.is_empty() {
            cum_slug.push_str(part);
        } else {
            cum_slug.push('.');
            cum_slug.push_str(part);
        }
        let b_title = if b_page_label == "name" {
            part.to_string()
        } else if b_page_label == "weight" {
            ctx.page_meta
                .get(&cum_slug)
                .map(|m| m.weight.clone())
                .unwrap_or_else(|| "9999".to_string())
        } else {
            ctx.page_meta
                .get(&cum_slug)
                .and_then(|m| if m.title.is_empty() { None } else { Some(m.title.clone()) })
                .unwrap_or_else(|| part.to_string())
        };
        let b_idx = i + 1;
        if i == b_parts.len() - 1 {
            out.push_str(&format!(
                "<span class=\"breadcrumb-sep\">{}</span><a href=\"/{}.html\" class=\"breadcrumb-current\">{}</a>",
                b_sep,
                cum_slug.replace('.', "/"),
                b_title
            ));
        } else {
            let dir_path = cum_slug.replace('.', "/");
            if ctx.cfg.source_dir.join(&dir_path).join("index.md").is_file()
                || ctx.cfg.source_dir.join(&dir_path).join("index.wiki").is_file()
            {
                out.push_str(&format!(
                    "<span class=\"breadcrumb-sep\">{}</span><a href=\"/{}/index.html\" class=\"breadcrumb-{}\">{}</a>",
                    b_sep, dir_path, b_idx, b_title
                ));
            } else if ctx.cfg.source_dir.join(format!("{}.md", dir_path)).is_file()
                || ctx.cfg.source_dir.join(format!("{}.wiki", dir_path)).is_file()
            {
                out.push_str(&format!(
                    "<span class=\"breadcrumb-sep\">{}</span><a href=\"/{}.html\" class=\"breadcrumb-{}\">{}</a>",
                    b_sep, dir_path, b_idx, b_title
                ));
            } else if let Some(best_p) = ctx.dir_best_page.get(&cum_slug) {
                out.push_str(&format!(
                    "<span class=\"breadcrumb-sep\">{}</span><a href=\"/{}.html\" class=\"breadcrumb-{}\">{}</a>",
                    b_sep,
                    best_p.replace('.', "/"),
                    b_idx,
                    b_title
                ));
            } else {
                out.push_str(&format!(
                    "<span class=\"breadcrumb-sep\">{}</span><span class=\"breadcrumb-{}\">{}</span>",
                    b_sep, b_idx, b_title
                ));
            }
        }
    }
    out.push_str("</breadcrumb>");
    out
}

pub fn partial_head(state: &RenderState, args: &str) -> String {
    let mut h_title = String::new();
    let mut h_css = String::new();
    let mut h_js = String::new();
    let mut h_thumb = String::new();

    for (k, mut val) in parse_kv_args(args) {
        if val.starts_with("page.") {
            val = state.page_vars.get(&val).cloned().unwrap_or_default();
        }
        match k.as_str() {
            "title" => h_title = val,
            "css" => h_css = val,
            "js" => h_js = val,
            "thumb" => h_thumb = val,
            _ => {}
        }
    }

    if !h_thumb.is_empty()
        && !h_thumb.starts_with("http://")
        && !h_thumb.starts_with("https://")
        && !h_thumb.starts_with('/')
    {
        h_thumb = format!("/_assets/{}", h_thumb);
    }

    let mut out = format!(
        "<meta charset=\"UTF-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n<title>{}</title>\n",
        h_title
    );

    let css_items: Vec<&str> = if h_css.is_empty() {
        vec![""]
    } else {
        h_css.split(',').collect()
    };
    for item in css_items {
        out.push_str(&format!("<link rel=\"stylesheet\" href=\"{}\">\n", item));
    }

    out.push_str("<link rel=\"icon\" type=\"image/png\" href=\"/_static/favicon/favicon-96x96.png\" sizes=\"96x96\">\n");
    out.push_str("<link rel=\"icon\" type=\"image/svg+xml\" href=\"/_static/favicon/favicon.svg\">\n");
    out.push_str("<link rel=\"shortcut icon\" href=\"/_static/favicon/favicon.ico\">\n");
    out.push_str("<link rel=\"apple-touch-icon\" sizes=\"180x180\" href=\"/_static/favicon/apple-touch-icon.png\">\n");
    out.push_str("<meta name=\"apple-mobile-web-app-title\" content=\"ximenesyuri\">\n");
    out.push_str("<link rel=\"manifest\" href=\"/_static/favicon/site.webmanifest\">\n");
    out.push_str(&format!("<script type=\"module\" src=\"{}\"></script>\n", h_js));

    if !h_thumb.is_empty() {
        out.push_str(&format!("<meta property=\"og:title\" content=\"{}\">\n", h_title));
        out.push_str(&format!("<meta property=\"og:image\" content=\"{}\">\n", h_thumb));
        out.push_str("<meta name=\"twitter:card\" content=\"summary_large_image\">\n");
        out.push_str(&format!("<meta name=\"twitter:title\" content=\"{}\">\n", h_title));
        out.push_str(&format!("<meta name=\"twitter:image\" content=\"{}\">\n", h_thumb));
    }

    out.push_str(
        "<script>
        window.addEventListener('hashchange', function() {
            var m = document.getElementById('search-modal');
            if (m) m.style.display = 'none';
            var r = document.getElementById('search-results');
            if (r) r.style.display = 'none';
        });
        document.addEventListener('DOMContentLoaded', async function() {
            if (window.matchMedia(\"(max-width: 768px)\").matches) {
                document.querySelectorAll('.menu-details > details[open]').forEach(function(details){
                    details.removeAttribute('open');
                });
            }
            wiki.full(true);
        });
    </script>\n</head>",
    );
    out
}

pub fn partial_author(state: &RenderState, args: &str) -> String {
    let mut a_prefix = String::new();
    for (k, v) in parse_kv_args(args) {
        if k == "prefix" {
            a_prefix = v;
        }
    }
    let Some(author_val) = state.page_vars.get("page.author").filter(|s| !s.is_empty()) else {
        return String::new();
    };

    let (a_name, a_url) = if let Some((n, u)) = state.ctx.omni.authors.get(author_val) {
        (n.clone(), u.clone())
    } else {
        let mut n = String::new();
        let mut u = String::new();
        for (k, v) in parse_kv_args(author_val) {
            if k == "name" {
                n = v;
            } else if k == "url" {
                u = v;
            }
        }
        if n.is_empty() {
            n = author_val.clone();
        }
        (n, u)
    };
    format!(
        "<author>\n<span class=\"author-prefix\">{}</span><a class=\"author-link\" href=\"{}\">{}</a>\n</author>",
        a_prefix, a_url, a_name
    )
}

pub fn partial_search(args: &str) -> String {
    let mut s_modal = false;
    let mut s_page = false;
    let mut s_placeholder = String::new();

    for (k, v) in parse_kv_args(args) {
        match k.as_str() {
            "modal" => s_modal = v.eq_ignore_ascii_case("true") || v == "1",
            "page" => s_page = v.eq_ignore_ascii_case("true") || v == "1",
            "placeholder" => s_placeholder = v,
            _ => {}
        }
    }

    let (ph_page, ph_modal, ph_normal) = if !s_placeholder.is_empty() {
        let p = format!(" placeholder=\"{}\"", s_placeholder);
        (p.clone(), p.clone(), p)
    } else {
        (
            String::new(),
            " placeholder=\"search...\"".to_string(),
            String::new(),
        )
    };
    let close_js = " onclick=\"if(event.target.closest('a')){this.style.display='none';var m=document.getElementById('search-modal');if(m)m.style.display='none';}\"";

    if s_page {
        format!(
            "<div class=\"search-page-container\">\n    <form action=\"/search.html\" method=\"GET\" class=\"search-form\">\n        <span class=\"search-input-prefix\">search:</span> <input type=\"text\" name=\"q\" id=\"search-page-input\" autocomplete=\"off\"{}>\n    </form>\n    <div id=\"search-page-results\" class=\"search-inline\"></div>\n</div>",
            ph_page
        )
    } else if s_modal {
        format!(
            "<a href=\"#\" id=\"search-toggle\">search</a>\n<div id=\"search-modal\" class=\"search-modal\" style=\"display: none;\">\n    <div class=\"search-modal-content\">\n        <input type=\"text\" name=\"q\" id=\"search-input\" autocomplete=\"off\"{}>\n        <div id=\"search-results\" class=\"search-popup\" style=\"display: none;\" {}></div>\n    </div>\n</div>",
            ph_modal,
            close_js.trim_start()
        )
    } else {
        format!(
            "<div class=\"search-container\">\n    <form action=\"/search.html\" method=\"GET\" class=\"search-form\" style=\"display: inline;\">\n        <span class=\"search-input-prefix\">search:</span> <input type=\"text\" name=\"q\" id=\"search-input\" autocomplete=\"off\"{}>\n    </form>\n    <div id=\"search-results\" class=\"search-popup\" style=\"display: none; position: absolute; z-index: 1000;\"{}></div>\n</div>",
            ph_normal, close_js
        )
    }
}

pub fn partial_thumb(state: &RenderState) -> String {
    let mut t_img = state
        .page_vars
        .get("page.thumb")
        .or_else(|| state.page_vars.get("thumb"))
        .or_else(|| state.ctx.page_meta.get(&state.pagename).map(|m| &m.thumb))
        .cloned()
        .unwrap_or_default();

    if t_img.is_empty() {
        return String::new();
    }
    if let Some(stripped) = t_img.strip_prefix("./") {
        let dir_path = if state.current_dir == "." || state.current_dir == "/" {
            String::new()
        } else {
            state.current_dir.replace('.', "/")
        };
        let dot = stripped.rfind('.').unwrap_or(stripped.len());
        let stem = &stripped[..dot];
        t_img = if dir_path.is_empty() {
            format!("/_assets/{}.webp", stem)
        } else {
            format!("/_assets/{}/{}.webp", dir_path, stem)
        };
    } else if !t_img.starts_with("http://")
        && !t_img.starts_with("https://")
        && !t_img.starts_with('/')
    {
        if !t_img.contains('.') {
            let base = state.ctx.cfg.build_dir.join("_assets");
            for ext in ["webp", "png", "jpg", "jpeg"] {
                if base.join(format!("{}.{}", t_img, ext)).exists() {
                    t_img = format!("{}.{}", t_img, ext);
                    break;
                }
            }
        }
        t_img = format!("/_assets/{}", t_img);
    }

    format!(
        "<figure class=\"block-image\" style=\"display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center;\">\n    <img src=\"{}\" alt=\"\" class=\"block-image-img\" style=\"width: inherit;\">\n</figure>",
        t_img
    )
}

pub fn partial_tags(state: &RenderState, args: &str) -> String {
    let mut t_prefix = "tags:".to_string();
    for (k, v) in parse_kv_args(args) {
        if k == "prefix" {
            t_prefix = v;
        }
    }
    let t_tags = state
        .page_vars
        .get("page.tags")
        .or_else(|| state.page_vars.get("tags"))
        .or_else(|| state.ctx.page_meta.get(&state.pagename).map(|m| &m.tags))
        .cloned()
        .unwrap_or_default();

    if t_tags.is_empty() {
        return String::new();
    }
    let mut html = format!("<tags>\n<prefix class=\"tags-prefix\">{}</prefix>\n", t_prefix);
    for t in t_tags.split(',') {
        let t = t.trim();
        if !t.is_empty() {
            html.push_str(&format!("<tag><a href=\"?search=tag:{}\">{}</a></tag>\n", t, t));
        }
    }
    html.push_str("</tags>");
    html
}

pub fn partial_related(state: &RenderState, args: &str) -> String {
    let mut title = String::new();
    let mut pass_args = String::from("root=/");
    for (k, v) in parse_kv_args(args) {
        if k == "title" {
            title = v;
        } else {
            pass_args.push_str(&format!(" {}=\"{}\"", k, v));
        }
    }
    let rel_val = state
        .page_vars
        .get("page.related")
        .or_else(|| state.page_vars.get("related"))
        .or_else(|| state.ctx.page_meta.get(&state.pagename).map(|m| &m.related))
        .cloned()
        .unwrap_or_default();
    if !rel_val.is_empty() {
        pass_args.push(' ');
        pass_args.push_str(&rel_val);
    }
    let mut out = String::new();
    if !title.is_empty() {
        out.push_str(&format!("<h1>{}</h1>\n", title));
    }
    out.push_str(&listing_pages(state.ctx, &state.current_dir, &pass_args));
    out
}

fn resolve_icon_html(ctx: &WikiContext, icon: &str) -> String {
    if (icon.starts_with("http://") || icon.starts_with("https://")) && icon.ends_with(".svg") {
        if let Ok(out) = Command::new("curl").args(["-sL", icon]).output() {
            if out.status.success() {
                return String::from_utf8_lossy(&out.stdout).into_owned();
            }
        }
        format!("<img src=\"{}\" alt=\"icon\"/>", icon)
    } else if icon.ends_with(".svg") {
        let p1 = std::path::Path::new(icon);
        let p2 = ctx.cfg.source_dir.join(icon);
        let p3 = ctx.cfg.static_dir.join(icon.trim_start_matches("/_static/"));
        if let Ok(s) = fs::read_to_string(p1)
            .or_else(|_| fs::read_to_string(&p2))
            .or_else(|_| fs::read_to_string(&p3))
        {
            s
        } else {
            format!("<img src=\"{}\" alt=\"icon\"/>", icon)
        }
    } else {
        format!("<img src=\"{}\" alt=\"icon\"/>", icon)
    }
}

pub fn partial_settings(ctx: &WikiContext, args: &str) -> String {
    let mut s_label = String::new();
    let mut s_icon = String::new();
    let mut s_content_align = false;
    let mut s_content_align_desc = String::new();
    let mut s_button = true;
    let mut s_themes_selector = false;
    let mut s_themes_path = String::new();
    let mut s_themes_desc = "tema".to_string();

    for (k, v) in parse_kv_args(args) {
        match k.as_str() {
            "label" => s_label = v,
            "icon" => s_icon = v,
            "content_align" => s_content_align = v.eq_ignore_ascii_case("true"),
            "content_align_desc" => s_content_align_desc = v,
            "button" => s_button = !v.eq_ignore_ascii_case("false"),
            "themes_selector" => s_themes_selector = v.eq_ignore_ascii_case("true"),
            "themes_path" => s_themes_path = v,
            "themes_desc" => s_themes_desc = v,
            _ => {}
        }
    }

    let mut btn_html = String::new();
    if !s_icon.is_empty() {
        btn_html.push_str(&resolve_icon_html(ctx, &s_icon));
    }
    if !s_label.is_empty() {
        if !btn_html.is_empty() {
            btn_html.push(' ');
        }
        btn_html.push_str(&format!("<span>{}</span>", s_label));
    }

    let (tag_open, tag_close) = if s_button {
        (
            "<button type=\"button\" id=\"settings-toggle\" aria-label=\"configurações\" aria-expanded=\"false\" aria-controls=\"settings-modal\">",
            "</button>",
        )
    } else {
        (
            "<a href=\"#\" id=\"settings-toggle\" aria-label=\"configurações\" aria-expanded=\"false\" aria-controls=\"settings-modal\">",
            "</a>",
        )
    };

    let mut html = format!(
        "    {}\n        {}\n    {}\n    <div id=\"settings-modal\" class=\"settings-modal\" role=\"dialog\" aria-label=\"configurações\" style=\"display: none;\">\n        <div class=\"settings-modal-content\">\n",
        tag_open, btn_html, tag_close
    );

    if s_content_align {
        html.push_str(&format!(
            "            <div class=\"settings-row\" data-setting=\"content-align\">\n                <span class=\"settings-label\" id=\"settings-content-align\">{}</span>\n                <div class=\"settings-segmented\" role=\"radiogroup\" aria-labelledby=\"settings-content-align\">\n                <button type=\"button\" role=\"radio\" data-value=\"left\" aria-label=\"à esquerda\" aria-checked=\"false\" tabindex=\"-1\">\n                    <svg viewBox=\"0 0 20 16\" width=\"20\" height=\"16\" aria-hidden=\"true\"><rect x=\"0.5\" y=\"0.5\" width=\"19\" height=\"15\" fill=\"none\" stroke=\"currentColor\"/><rect x=\"3\" y=\"4\" width=\"5\" height=\"8\" fill=\"currentColor\"/></svg>\n                </button>\n                <button type=\"button\" role=\"radio\" data-value=\"center\" aria-label=\"centralizado\" aria-checked=\"false\" tabindex=\"-1\">\n                    <svg viewBox=\"0 0 20 16\" width=\"20\" height=\"16\" aria-hidden=\"true\"><rect x=\"0.5\" y=\"0.5\" width=\"19\" height=\"15\" fill=\"none\" stroke=\"currentColor\"/><rect x=\"7.5\" y=\"4\" width=\"5\" height=\"8\" fill=\"currentColor\"/></svg>\n                </button>\n                <button type=\"button\" role=\"radio\" data-value=\"right\" aria-label=\"à direita\" aria-checked=\"false\" tabindex=\"-1\">\n                    <svg viewBox=\"0 0 20 16\" width=\"20\" height=\"16\" aria-hidden=\"true\"><rect x=\"0.5\" y=\"0.5\" width=\"19\" height=\"15\" fill=\"none\" stroke=\"currentColor\"/><rect x=\"12\" y=\"4\" width=\"5\" height=\"8\" fill=\"currentColor\"/></svg>\n                </button>\n                </div>\n            </div>\n",
            s_content_align_desc
        ));
    }

    if s_themes_selector {
        html.push_str(&format!(
            "            <div class=\"settings-row\" data-setting=\"theme\">\n                <span class=\"settings-label\" id=\"settings-theme\">{}</span>\n                <div class=\"settings-segmented\">\n                    <select id=\"lemma-theme-select\" data-path=\"{}\" aria-labelledby=\"settings-theme\" style=\"background:transparent;color:inherit;border:none;outline:none;cursor:pointer;padding:5px 7px;\"></select>\n                </div>\n            </div>\n",
            s_themes_desc, s_themes_path
        ));
        if !s_themes_path.is_empty() {
            let abs_path = if s_themes_path.starts_with('/') {
                let p1 = ctx
                    .cfg
                    .build_dir
                    .join(s_themes_path.trim_start_matches('/'));
                if p1.is_file() {
                    p1
                } else if let Some(rel) = s_themes_path.strip_prefix("/_static/") {
                    ctx.cfg.static_dir.join(rel)
                } else {
                    p1
                }
            } else {
                ctx.cfg.source_dir.join(&s_themes_path)
            };
            if let Ok(json_str) = fs::read_to_string(abs_path) {
                html.push_str(&format!(
                    "            <script id=\"lemma-themes-data\" type=\"application/json\">{}</script>\n",
                    json_str
                ));
            }
        }
    }
    html.push_str("        </div>\n    </div>\n");
    html
}

pub fn partial_help(ctx: &WikiContext, args: &str) -> String {
    let mut h_button = false;
    let mut h_label = String::new();
    let mut h_icon = String::new();
    let mut keys: Vec<(String, String)> = Vec::new();

    for (k, v) in parse_kv_args(args) {
        match k.as_str() {
            "button" => h_button = v.eq_ignore_ascii_case("true") || v == "1",
            "label" => h_label = v,
            "icon" => h_icon = v,
            _ => keys.push((k, v)),
        }
    }

    let mut btn_html = String::new();
    if !h_icon.is_empty() {
        btn_html.push_str(&resolve_icon_html(ctx, &h_icon));
    }
    if !h_label.is_empty() {
        if !btn_html.is_empty() {
            btn_html.push(' ');
        }
        btn_html.push_str(&format!("<span>{}</span>", h_label));
    }

    let mut html = if h_button {
        format!(
            "    <button type=\"button\" id=\"help-toggle\" aria-label=\"help\" aria-expanded=\"false\" aria-controls=\"help-modal\">\n        {}\n    </button>\n",
            btn_html
        )
    } else {
        format!(
            "    <a href=\"#\" id=\"help-toggle\" aria-label=\"help\" aria-expanded=\"false\" aria-controls=\"help-modal\">\n        {}\n    </a>\n",
            btn_html
        )
    };

    html.push_str("    <div id=\"help-modal\" class=\"help-modal\" role=\"dialog\" aria-label=\"help\" style=\"display: none;\">\n        <div class=\"help-modal-content\">\n");
    for (k, desc) in keys {
        html.push_str(&format!(
            "            <div class=\"help-row\">\n                <span class=\"help-label\"><inline>{}</inline></span>\n                <span>{}</span>\n            </div>\n",
            k, desc
        ));
    }
    html.push_str("        </div>\n    </div>\n");
    html
}

pub fn render_template(state: &RenderState, raw_layout: &str, body_content: &str) -> String {
    let mut layout_out: Vec<String> = Vec::new();
    let mut skip = false;

    for raw_ll in raw_layout.lines() {
        let mut ll = raw_ll.trim_end_matches('\r').to_string();
        if ll.contains("{%") {
            let trimmed = ll.trim();
            if let Some(inner) = trimmed
                .strip_prefix("{%")
                .and_then(|s| s.strip_suffix("%}"))
                .map(|s| s.trim())
            {
                if let Some((sub, not_eq)) = parse_if_in_and_not_eq(inner) {
                    skip = !(state.pagename.contains(&sub) && state.pagename != not_eq);
                    continue;
                } else if let Some(sub) = parse_if_in_page_name(inner) {
                    skip = !state.pagename.contains(&sub);
                    continue;
                } else if inner == "else" {
                    skip = !skip;
                    continue;
                } else if inner == "endif" {
                    skip = false;
                    continue;
                } else if inner.starts_with("for ")
                    || inner.starts_with("from ")
                    || inner.starts_with("include ")
                    || inner.starts_with("endfor")
                {
                    continue;
                }
            }
        }
        if skip {
            continue;
        }
        if ll.contains("{{ render_menu_item") || ll.contains("{{ item") {
            continue;
        }

        if ll.contains("{{ page.") || ll.contains("{{page.") {
            ll = expand_page_vars(&ll, &state.page_vars);
        }
        if ll.contains("wiki.frontmatter.") {
            ll = expand_frontmatter_vars(&ll, &state.page_vars);
        }
        layout_out.push(ll);
    }

    let mut p_breadcrumb = true;
    let mut p_featured = true;
    let mut p_menu = true;
    if let Some(p_str) = state.page_vars.get("page.partials") {
        for (k, v) in parse_kv_args(p_str) {
            let b = !(v == "false" || v == "0");
            match k.as_str() {
                "breadcrumb" => p_breadcrumb = b,
                "featured" => p_featured = b,
                "menu" => p_menu = b,
                _ => {}
            }
        }
    }

    let mut rendered = format!("{}\n", layout_out.join("\n"));

    // Expand parameterized {{ [wiki.]partial args }}
    rendered = replace_partial_calls(&rendered, "menu", |args| {
        if p_menu {
            partial_menu(state.ctx, &state.pagename, args)
        } else {
            String::new()
        }
    });
    rendered = replace_partial_calls(&rendered, "breadcrumb", |args| {
        if p_breadcrumb {
            partial_breadcrumb(state.ctx, &state.pagename, args)
        } else {
            String::new()
        }
    });
    rendered = replace_partial_calls(&rendered, "related", |args| partial_related(state, args));
    rendered = replace_partial_calls(&rendered, "head", |args| partial_head(state, args));
    rendered = replace_partial_calls(&rendered, "author", |args| partial_author(state, args));
    rendered = replace_partial_calls(&rendered, "search", |args| partial_search(args));
    rendered =
        replace_partial_calls(&rendered, "settings", |args| partial_settings(state.ctx, args));
    rendered = replace_partial_calls(&rendered, "help", |args| partial_help(state.ctx, args));

    // Simple fixed tags
    if rendered.contains("{{ wiki.search }}") {
        let v = partial_search("");
        rendered = rendered.replace("{{ wiki.search }}", &v);
    }
    if rendered.contains("{{ wiki.thumb }}") {
        let v = partial_thumb(state);
        rendered = rendered.replace("{{ wiki.thumb }}", &v);
    }
    if rendered.contains("{{ wiki.tags }}") {
        let v = partial_tags(state, "");
        rendered = rendered.replace("{{ wiki.tags }}", &v);
    }
    if rendered.contains("{{ wiki.author }}") {
        let v = partial_author(state, "");
        rendered = rendered.replace("{{ wiki.author }}", &v);
    }
    if rendered.contains("{{ wiki.settings }}") {
        let v = partial_settings(state.ctx, "");
        rendered = rendered.replace("{{ wiki.settings }}", &v);
    }
    if rendered.contains("{{ wiki.help }}") {
        let v = partial_help(state.ctx, "");
        rendered = rendered.replace("{{ wiki.help }}", &v);
    }
    if rendered.contains("{{ wiki.menu }}") {
        let v = if p_menu {
            partial_menu(state.ctx, &state.pagename, "")
        } else {
            String::new()
        };
        rendered = rendered.replace("{{ wiki.menu }}", &v);
    }
    if rendered.contains("{{ wiki.featured }}") {
        let v = if p_featured {
            state.ctx.wiki_featured_html.as_str()
        } else {
            ""
        };
        rendered = rendered.replace("{{ wiki.featured }}", v);
    }
    if rendered.contains("{{ datetime.year }}") {
        rendered = rendered.replace("{{ datetime.year }}", &state.ctx.cfg.build_year);
    }
    if rendered.contains("{{ datetime.now }}") {
        rendered = rendered.replace("{{ datetime.now }}", &state.ctx.cfg.build_now);
    }
    if rendered.contains("{{ page.name }}") {
        rendered = rendered.replace("{{ page.name }}", &state.pagename);
    }
    if rendered.contains("{{ page.title }}") {
        let ptitle = state
            .ctx
            .page_meta
            .get(&state.pagename)
            .and_then(|m| if m.title.is_empty() { None } else { Some(m.title.as_str()) })
            .unwrap_or(&state.pagename);
        rendered = rendered.replace("{{ page.title }}", ptitle);
    }
    if rendered.contains("{{ page.body }}") {
        rendered = rendered.replace("{{ page.body }}", body_content);
    }

    let auto_js =
        "<script>%%WIKI_SEARCH_CONFIG%%; if (typeof hljs !== 'undefined') hljs.highlightAll();</script>";
    if let Some(pos) = rendered.find("</body>") {
        rendered.insert_str(pos, &format!("{}\n", auto_js));
    } else {
        rendered.push_str(auto_js);
        rendered.push('\n');
    }

    if !state.ctx.search_js.is_empty() {
        rendered = rendered.replace("%%WIKI_SEARCH_CONFIG%%", &state.ctx.search_js);
    }
    rendered
}

fn parse_if_in_and_not_eq(inner: &str) -> Option<(String, String)> {
    let rest = inner.strip_prefix("if")?.trim_start();
    let rest = rest.strip_prefix('\'')?;
    let q1 = rest.find('\'')?;
    let sub = rest[..q1].to_string();
    let after1 = rest[q1 + 1..].trim_start();
    let after_in = after1.strip_prefix("in")?.trim_start();
    let after_pn = after_in.strip_prefix("page.name")?.trim_start();
    let after_and = after_pn.strip_prefix("and")?.trim_start();
    let after_not = after_and.strip_prefix("not")?.trim_start();
    let after_pn2 = after_not.strip_prefix("page.name")?.trim_start();
    let after_eq = after_pn2.strip_prefix("==")?.trim_start();
    let after_q = after_eq.strip_prefix('"')?;
    let q2 = after_q.find('"')?;
    let not_eq = after_q[..q2].to_string();
    Some((sub, not_eq))
}

fn parse_if_in_page_name(inner: &str) -> Option<String> {
    let rest = inner.strip_prefix("if")?.trim_start();
    let rest = rest.strip_prefix('\'')?;
    let q1 = rest.find('\'')?;
    let sub = rest[..q1].to_string();
    let after1 = rest[q1 + 1..].trim_start();
    let after_in = after1.strip_prefix("in")?.trim_start();
    if after_in == "page.name" {
        Some(sub)
    } else {
        None
    }
}

fn expand_page_vars(line: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rem = line;
    while let Some(i1) = rem.find("{{") {
        let pre = &rem[..i1];
        let rest = &rem[i1 + 2..];
        let Some(i2) = rest.find("}}") else {
            break;
        };
        let inner = rest[..i2].trim();
        if let Some(suffix) = inner.strip_prefix("page.") {
            if matches!(suffix, "name" | "body" | "title") {
                out.push_str(pre);
                out.push_str(&format!("{{{{ page.{} }}}}", suffix));
            } else {
                let val = vars.get(&format!("page.{}", suffix)).map(|s| s.as_str()).unwrap_or("");
                out.push_str(pre);
                out.push_str(val);
            }
            rem = &rest[i2 + 2..];
            continue;
        }
        out.push_str(pre);
        out.push_str("{{");
        out.push_str(&rest[..i2]);
        out.push_str("}}");
        rem = &rest[i2 + 2..];
    }
    out.push_str(rem);
    out
}

fn expand_frontmatter_vars(line: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rem = line;
    while let Some(i1) = rem.find("{{") {
        let pre = &rem[..i1];
        let rest = &rem[i1 + 2..];
        let Some(i2) = rest.find("}}") else {
            break;
        };
        let inner = rest[..i2].trim();
        if let Some(suffix) = inner.strip_prefix("wiki.frontmatter.") {
            let val = vars.get(&format!("page.{}", suffix)).map(|s| s.as_str()).unwrap_or("");
            out.push_str(pre);
            out.push_str(val);
            rem = &rest[i2 + 2..];
            continue;
        }
        out.push_str(pre);
        out.push_str("{{");
        out.push_str(&rest[..i2]);
        out.push_str("}}");
        rem = &rest[i2 + 2..];
    }
    out.push_str(rem);
    out
}

fn replace_partial_calls<F>(s: &str, name: &str, mut f: F) -> String
where
    F: FnMut(&str) -> String,
{
    let wiki_name = format!("wiki.{}", name);
    let mut out = String::with_capacity(s.len());
    let mut rem = s;

    while let Some(i1) = rem.find("{{") {
        let pre = &rem[..i1];
        let rest = &rem[i1 + 2..];
        let Some(i2) = rest.find("}}") else {
            break;
        };
        let inner = rest[..i2].trim();

        let matched_args = if let Some(after) = inner.strip_prefix(&wiki_name) {
            if after.is_empty() {
                if matches!(name, "breadcrumb" | "related") {
                    Some("")
                } else {
                    None
                }
            } else if after.starts_with(char::is_whitespace) {
                Some(after.trim())
            } else {
                None
            }
        } else if let Some(after) = inner.strip_prefix(name) {
            if after.is_empty() {
                if matches!(name, "breadcrumb" | "related") {
                    Some("")
                } else {
                    None
                }
            } else if after.starts_with(char::is_whitespace) {
                Some(after.trim())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(args) = matched_args {
            out.push_str(pre);
            out.push_str(&f(args));
            rem = &rest[i2 + 2..];
        } else {
            out.push_str(pre);
            out.push_str("{{");
            out.push_str(&rest[..i2]);
            out.push_str("}}");
            rem = &rest[i2 + 2..];
        }
    }
    out.push_str(rem);
    out
}
