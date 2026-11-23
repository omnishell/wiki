use crate::diagram::render_diagram;
use crate::model::{
    char_len, get_or_render_latex, get_style_attr, html_escape, json_escape, resolve_wiki_ref,
    slugify, strip_quotes, RenderState,
};
use crate::parser::{parse_block_field, parse_inline_code_only};
use std::collections::{HashMap, HashSet};
use std::process::Command;

pub fn dispatch_block(state: &mut RenderState, bname: &str, buffer: &[String]) -> String {
    match bname {
        "def" => render_block_core(state, "def", buffer),
        "table" => render_block_table(state, buffer),
        "diagram" => render_diagram(state, buffer, false, ""),
        "hide" => render_block_hide(state, buffer),
        "row" => render_block_row(state, buffer),
        "carousel" => render_block_carousel(state, buffer),
        "card" => render_block_card(state, buffer),
        "buttons" => render_block_buttons(state, buffer),
        "text" => render_block_text(state, buffer),
        "image" => render_block_image(state, buffer),
        "grid" => render_block_grid(state, buffer),
        "plain" => render_block_plain(buffer),
        "pre" => render_block_pre(buffer),
        "tex" => render_block_tex(state, buffer),
        "quote" => render_block_quote(state, buffer),
        "globals" | "comment" => String::new(),
        "blockquote" => {
            let mut s = String::from("<blockquote>\n");
            for l in buffer {
                s.push_str(l);
                s.push('\n');
            }
            s.push_str("</blockquote>\n");
            s
        }
        other => {
            if let Some(cls) = state.ctx.omni.custom_blocks.get(other).cloned() {
                let mut s = format!("<div class=\"{}\">\n", cls);
                for l in buffer {
                    s.push_str(l);
                    s.push('\n');
                }
                s.push_str("</div>\n");
                s
            } else {
                render_block_core(state, other, buffer)
            }
        }
    }
}

pub fn dispatch_nested_block(
    state: &mut RenderState,
    bname: &str,
    buffer: &[String],
    inherited_ctx: &str,
) -> String {
    if bname == "diagram" {
        render_diagram(state, buffer, true, inherited_ctx)
    } else {
        dispatch_block(state, bname, buffer)
    }
}

fn parse_kv_buffer(buffer: &[String], track_nesting: bool) -> (Vec<String>, HashMap<String, String>) {
    let mut keys: Vec<String> = Vec::new();
    let mut vals: HashMap<String, String> = HashMap::new();
    let mut current_key = String::new();
    let mut current_val = String::new();
    let mut nested_depth: usize = 0;

    for b_line in buffer {
        let mut l_trim = b_line.trim_start();
        if track_nesting {
            if let Some(after_col) = l_trim.strip_prefix("::") {
                let after_blank = after_col
                    .strip_prefix(' ')
                    .or_else(|| after_col.strip_prefix('\t'))
                    .unwrap_or(after_col);
                let sub_trim = after_blank.trim_start();
                if sub_trim.starts_with("{{") && sub_trim.contains("}}") {
                    l_trim = sub_trim;
                }
            }
            if l_trim.starts_with("{{") {
                let r_trim = l_trim.trim_end();
                if r_trim.ends_with("}}") {
                    let inner = r_trim[2..r_trim.len() - 2].trim();
                    if inner == "block.end" {
                        if nested_depth > 0 {
                            nested_depth -= 1;
                        }
                    } else if inner.starts_with("block.") && !r_trim.contains("block.end") {
                        nested_depth += 1;
                    }
                }
            }
        }

        if nested_depth == 0 && l_trim.starts_with("::") {
            let after_col = l_trim[2..].trim_start();
            if let Some(colon_idx) = after_col.find(':') {
                let k_part = &after_col[..colon_idx];
                if !k_part.is_empty()
                    && k_part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    if !current_key.is_empty() {
                        if !vals.contains_key(&current_key) {
                            keys.push(current_key.clone());
                        }
                        vals.insert(current_key, current_val);
                    }
                    current_key = k_part.to_string();
                    current_val = after_col[colon_idx + 1..].trim().to_string();
                    continue;
                }
            }
        }

        if !current_key.is_empty() {
            current_val.push('\n');
            current_val.push_str(b_line);
        }
    }

    if !current_key.is_empty() {
        if !vals.contains_key(&current_key) {
            keys.push(current_key.clone());
        }
        vals.insert(current_key, current_val);
    }

    (keys, vals)
}

pub fn render_block_core(state: &mut RenderState, bname: &str, buffer: &[String]) -> String {
    let (mut block_keys, mut block_vals) = parse_kv_buffer(buffer, true);
    let mut display_keys: HashMap<String, String> = HashMap::new();
    let mut display_vals: HashMap<String, String> = HashMap::new();

    for k in &block_keys {
        display_keys.insert(k.clone(), k.clone());
        if let Some(v) = block_vals.get(k) {
            display_vals.insert(k.clone(), v.clone());
        }
    }

    if bname == "def" {
        apply_def_hook(
            &mut block_keys,
            &mut block_vals,
            &mut display_keys,
            &mut display_vals,
        );
    }

    let block_entry = block_vals.get("__entry__").cloned().unwrap_or_default();
    let block_context = block_vals.get("__context__").cloned().unwrap_or_default();

    let mut first_content_key = String::new();
    let mut first_content_val = String::new();

    if let Some(t) = block_vals.get("term").filter(|s| !s.is_empty()) {
        first_content_key = "term".to_string();
        first_content_val = t.clone();
    } else if let Some(t) = block_vals.get("type").filter(|s| !s.is_empty()) {
        first_content_key = "type".to_string();
        first_content_val = t.clone();
    } else {
        for k in &block_keys {
            if !matches!(
                k.as_str(),
                "__id__" | "__context__" | "context" | "__entry__" | "__refs__"
            ) {
                first_content_key = k.clone();
                first_content_val = block_vals.get(k).cloned().unwrap_or_default();
                break;
            }
        }
    }

    let mut raw_id = block_vals.get("__id__").cloned().unwrap_or_default();
    if raw_id.is_empty() {
        raw_id = if !block_entry.is_empty() {
            block_entry.clone()
        } else {
            first_content_val.clone()
        };
    }
    if raw_id.is_empty() && !block_keys.is_empty() {
        raw_id = block_vals
            .get(&block_keys[0])
            .cloned()
            .unwrap_or_default();
    }

    let mut clean_id = slugify(&raw_id);
    if clean_id.is_empty() {
        clean_id = format!("rnd-{}", state.page_table_count + state.page_diagram_count + 1);
    }
    let ref_key = if !block_context.is_empty() && !raw_id.contains('.') {
        slugify(&format!("{}.{}", block_context, raw_id))
    } else {
        clean_id.clone()
    };

    let anchor_id_slug = ref_key.replace(['/', '.'], "-");
    let anchor_id = format!("block-{}-{}", bname, anchor_id_slug);
    let block_title = if !block_entry.is_empty() {
        block_entry
    } else if !first_content_val.is_empty() {
        first_content_val.clone()
    } else {
        raw_id.clone()
    };

    if !block_context.is_empty() {
        let mut first_dk = None;
        for k in &block_keys {
            if let Some(dk) = display_keys.get(k) {
                if !dk.is_empty()
                    && !matches!(
                        k.as_str(),
                        "__id__" | "__context__" | "__entry__" | "__refs__"
                    )
                {
                    first_dk = Some(k.clone());
                    break;
                }
            }
        }
        if let Some(fdk) = first_dk {
            if let Some(dv) = display_vals.get_mut(&fdk) {
                dv.push_str(&format!(
                    " <span class=\"block-context-delimiter\">(</span><span class=\"block-context\">{}</span><span class=\"block-context-delimiter\">)</span>",
                    block_context
                ));
            }
        }
    }

    let mut html_out = String::with_capacity(512);
    html_out.push_str(&format!(
        "<block id=\"{}\" class=\"block-{}\">\n    <fields id=\"fields-{}-{}\" class=\"fields-{}\">\n",
        anchor_id, bname, bname, anchor_id_slug, bname
    ));

    let page_url = format!("/{}.html#{}", state.pagename.replace('.', "/"), anchor_id);
    let mut json_s = format!("{{\"url\":\"{}\"", json_escape(&page_url));
    if !block_context.is_empty() {
        json_s.push_str(&format!(",\"__context__\":\"{}\"", json_escape(&block_context)));
    }

    for k in &block_keys {
        if matches!(k.as_str(), "__id__" | "__context__" | "__entry__") {
            continue;
        }
        let orig_v = block_vals.get(k).cloned().unwrap_or_default();
        json_s.push_str(&format!(",\"{}\":\"{}\"", k, json_escape(&orig_v)));

        let dk_raw = display_keys.get(k).cloned().unwrap_or_default();
        if dk_raw.is_empty() {
            continue;
        }
        let mut dk = dk_raw;
        let mut dv = display_vals.get(k).cloned().unwrap_or_default();

        if k == "__refs__" || dk == "__refs__" {
            let prefix_name = if k != "__refs__" {
                k.as_str()
            } else {
                "refs"
            };
            let mut rel_out = String::from("<ul>\n");
            for r_item in dv.split(',') {
                let r_item = r_item.trim();
                if r_item.is_empty() {
                    continue;
                }
                if let Some(dot) = r_item.find('.') {
                    let r_topic = &r_item[..dot];
                    let r_ref = &r_item[dot + 1..];
                    let slug_ref = slugify(r_ref);
                    if r_topic == "page" {
                        rel_out.push_str(&format!(
                            "        <li><a href=\"/{}.html\">{}</a></li>\n",
                            slug_ref.replace('.', "/"),
                            r_ref
                        ));
                    } else {
                        let resolved = resolve_wiki_ref(state.ctx, r_topic, &slug_ref, r_ref);
                        rel_out.push_str(&format!("        <li>{}</li>\n", resolved));
                    }
                }
            }
            rel_out.push_str("    </ul>");
            dv = rel_out;
            dk = prefix_name.to_string();
        } else if orig_v == dv {
            dv = parse_block_field(state, &dv, &block_context);
        } else {
            // Even when wrapped in <u>...</u> or context span, parse inline refs/tex if needed
        }

        html_out.push_str(&format!(
            "        <field id=\"field-{}-{}-{}\" class=\"field-{}-{}\">\n            <prefix id=\"prefix-{}-{}-{}\" class=\"prefix-{}-{}\">{}: </prefix><div id=\"text-{}-{}-{}\" class=\"text-prefix-{}-{}\">{}</div>\n        </field>\n",
            bname, dk, anchor_id_slug, bname, dk,
            bname, dk, anchor_id_slug, bname, dk, dk,
            bname, dk, anchor_id_slug, bname, dk, dv
        ));
    }

    html_out.push_str("    </fields>\n</block>\n");
    json_s.push_str("},\n");

    state
        .search_index_map
        .entry(bname.to_string())
        .or_default()
        .push_str(&json_s);

    if !state.page_title_keys.contains_key(bname) {
        let tk = if !first_content_key.is_empty() {
            first_content_key
        } else if !block_keys.is_empty() {
            block_keys[0].clone()
        } else {
            bname.to_string()
        };
        state.page_title_keys.insert(bname.to_string(), tk);
    }

    state
        .list_html_map
        .entry(bname.to_string())
        .or_default()
        .push_str(&format!(
            "<li><a href=\"#{}\">{}</a></li>\n",
            anchor_id, block_title
        ));

    html_out
}

fn apply_def_hook(
    block_keys: &mut Vec<String>,
    block_vals: &mut HashMap<String, String>,
    display_keys: &mut HashMap<String, String>,
    display_vals: &mut HashMap<String, String>,
) {
    if display_keys.get("term").map(|s| !s.is_empty()).unwrap_or(false) {
        display_keys.insert("term".to_string(), "def".to_string());
        if let Some(t) = block_vals.get("term") {
            display_vals.insert("term".to_string(), format!("<u>{}</u>", t));
        }
    }
    if let Some(ctx) = block_vals.get("context").filter(|s| !s.is_empty()).cloned() {
        block_vals.insert("__context__".to_string(), ctx);
        display_keys.insert("context".to_string(), String::new());
    }

    let def_ctx = block_vals.get("__context__").cloned().unwrap_or_default();
    let self_raw = block_vals
        .get("__id__")
        .or_else(|| block_vals.get("__entry__"))
        .or_else(|| block_vals.get("term"))
        .cloned()
        .unwrap_or_default();

    let self_slug = if !self_raw.is_empty() {
        if !def_ctx.is_empty() && !self_raw.contains('.') {
            slugify(&format!("{}.{}", def_ctx, self_raw))
        } else {
            slugify(&self_raw)
        }
    } else {
        String::new()
    };

    let has_related = block_vals.get("related").map(|s| !s.is_empty()).unwrap_or(false);
    let has_refs = block_vals.get("__refs__").map(|s| !s.is_empty()).unwrap_or(false);

    if !has_related && !has_refs {
        let idea = block_vals.get("idea").map(|s| s.as_str()).unwrap_or("");
        let details = block_vals.get("details").map(|s| s.as_str()).unwrap_or("");
        let scan_str = format!("{}\n{}", idea, details);

        if scan_str.contains("{{") && scan_str.contains("}}") {
            let mut extracted: Vec<String> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            let mut rem = scan_str.as_str();

            while let Some(i1) = rem.find("{{") {
                rem = &rem[i1 + 2..];
                let Some(i2) = rem.find("}}") else { break };
                let inner = rem[..i2].trim();
                rem = &rem[i2 + 2..];

                if let Some(colon) = inner.find(':') {
                    let t_topic = &inner[..colon];
                    if !t_topic.is_empty()
                        && t_topic
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                        && t_topic != "page"
                        && t_topic != "block"
                        && t_topic != "diagram"
                    {
                        let t_rest = &inner[colon + 1..];
                        let t_ref_raw = t_rest.split(':').next().unwrap_or("").trim();
                        if !t_ref_raw.is_empty() {
                            let mut t_ref = t_ref_raw.to_string();
                            if t_topic == "def" && !def_ctx.is_empty() && !t_ref.contains('.') {
                                t_ref = format!("{}.{}", def_ctx, t_ref);
                            }
                            let ref_slug = slugify(&t_ref);
                            if t_topic == "def" && !self_slug.is_empty() && ref_slug == self_slug {
                                continue;
                            }
                            let full_ref = format!("{}.{}", t_topic, t_ref);
                            if seen.insert(full_ref.clone()) {
                                extracted.push(full_ref);
                            }
                        }
                    }
                }
            }

            if !extracted.is_empty() {
                let joined = extracted.join(", ");
                block_vals.insert("related".to_string(), joined.clone());
                if !display_keys.contains_key("related") {
                    block_keys.push("related".to_string());
                }
                display_keys.insert("related".to_string(), "related".to_string());
                display_vals.insert("related".to_string(), joined);
            }
        }
    }

    if let Some(rel) = block_vals.get("related").filter(|s| !s.is_empty()).cloned() {
        let mut new_items = Vec::new();
        for item in rel.split(',') {
            let r_item = item.trim();
            if r_item.is_empty() {
                continue;
            }
            let dot_count = r_item.matches('.').count();
            if dot_count < 2 && !r_item.starts_with("def.") {
                new_items.push(format!("def.{}", r_item));
            } else {
                new_items.push(r_item.to_string());
            }
        }
        display_vals.insert("related".to_string(), new_items.join(", "));
        display_keys.insert("related".to_string(), "__refs__".to_string());
    }
}

pub fn render_block_table(state: &mut RenderState, raw_buffer: &[String]) -> String {
    if raw_buffer.len() < 3 {
        return String::new();
    }
    let mut cleaned: Vec<String> = Vec::with_capacity(raw_buffer.len());
    let mut sep_indices: Vec<usize> = Vec::new();

    for (i, line) in raw_buffer.iter().enumerate() {
        let mut tmp = line.replace('\t', "    ");
        tmp = tmp.replace('\u{00A0}', " ");
        tmp.retain(|c| !c.is_control());
        let no_spaces: String = tmp.chars().filter(|&c| c != ' ').collect();
        if !no_spaces.is_empty() && no_spaces.chars().all(|c| matches!(c, '-' | '=' | '_')) {
            sep_indices.push(i);
        }
        cleaned.push(tmp);
    }

    if sep_indices.is_empty() {
        return String::new();
    }

    let sep1 = sep_indices[0];
    let mut sep2 = cleaned.len();
    let mut caption_line = String::new();

    if sep_indices.len() >= 2 {
        sep2 = *sep_indices.last().unwrap();
        for item in cleaned.iter().skip(sep2 + 1) {
            let cl = item.trim_matches(' ');
            if !cl.is_empty() {
                caption_line = cl.to_string();
                break;
            }
        }
    }

    let header_line = cleaned[0].trim_matches(' ');
    let headers = split_two_spaces(header_line);
    let num_columns = headers.len();
    if num_columns == 0 {
        return String::new();
    }

    let mut max_widths: Vec<usize> = headers.iter().map(|h| char_len(h)).collect();
    let mut parsed_rows: Vec<Vec<String>> = Vec::new();

    for item in cleaned.iter().take(sep2).skip(sep1 + 1) {
        let row_str = item.trim_matches(' ');
        if row_str.is_empty() {
            continue;
        }
        let mut cols = split_two_spaces(row_str);
        if cols.len() > num_columns {
            let rest = cols[num_columns - 1..].join(" ");
            cols.truncate(num_columns - 1);
            cols.push(rest);
        }
        for (j, col) in cols.iter().enumerate().take(num_columns) {
            let flen = char_len(col);
            if flen > max_widths[j] {
                max_widths[j] = flen;
            }
        }
        parsed_rows.push(cols);
    }

    state.page_table_count += 1;
    let table_id = format!("table-{}", state.page_table_count);

    let mut sep_row_html = String::from("<tr class=\"ascii-separator\">");
    for &mw in &max_widths {
        let dashes = "-".repeat(mw + 4);
        sep_row_html.push_str(&format!("<td><p>{}</p></td>", dashes));
    }
    sep_row_html.push_str("</tr>\n");

    let mut table_html = format!(
        "<div id=\"{}\" class=\"captioned-table\">\n<table class=\"custom-pre-table\">\n<thead>\n<tr>",
        table_id
    );

    for h in &headers {
        let f = parse_inline_code_only(&html_escape(h.trim_matches(' ')));
        table_html.push_str(&format!("<th><p>{}</p></th>", f));
    }
    table_html.push_str("</tr>\n</thead>\n<tbody>\n");
    table_html.push_str(&sep_row_html);

    for cols in &parsed_rows {
        table_html.push_str("<tr>");
        for j in 0..num_columns {
            let raw_c = cols.get(j).map(|s| s.as_str()).unwrap_or("");
            let f = parse_inline_code_only(&html_escape(raw_c.trim_matches(' ')));
            table_html.push_str(&format!("<td><p>{}</p></td>", f));
        }
        table_html.push_str("</tr>\n");
    }

    if sep_indices.len() >= 2 {
        table_html.push_str(&sep_row_html);
    }
    table_html.push_str("</tbody>\n</table>\n");

    let safe_cap = html_escape(&caption_line);
    if !safe_cap.is_empty() {
        table_html.push_str(&format!(
            "<p><a href=\"#\" class=\"table-caption\">{}</a></p>\n",
            safe_cap
        ));
    }
    table_html.push_str("</div>\n");

    let enc_url = json_escape(&format!("/{}.html", state.pagename.replace('.', "/")));
    let enc_headers = json_escape(header_line);
    let enc_lines = json_escape(&cleaned.join(" "));
    state
        .search_index_map
        .entry("table".to_string())
        .or_default()
        .push_str(&format!(
            "{{\"url\":\"{}\",\"headers\":\"{}\",\"columns\":\"{}\",\"lines\":\"{}\"}},\n",
            enc_url, enc_headers, num_columns, enc_lines
        ));

    let list_label = if safe_cap.is_empty() {
        format!("Table {}", state.page_table_count)
    } else {
        safe_cap
    };
    state
        .list_html_map
        .entry("table".to_string())
        .or_default()
        .push_str(&format!("<li><a href=\"#{}\">{}</a></li>\n", table_id, list_label));

    table_html
}

fn split_two_spaces(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ' ' && i + 1 < chars.len() && chars[i + 1] == ' ' {
            let trimmed = cur.trim_matches(' ');
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
            cur.clear();
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
        } else {
            cur.push(chars[i]);
            i += 1;
        }
    }
    let trimmed = cur.trim_matches(' ');
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    out
}

pub fn render_block_plain(buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    format!(
        "<plain style=\"white-space: pre; overflow-x: auto; display: block;\">\n{}\n</plain>\n",
        buffer.join("\n")
    )
}

pub fn render_block_pre(buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    format!(
        "<pre class=\"block-pre\" style=\"white-space: pre; overflow-x: auto; display: block;\">\n{}\n</pre>\n",
        html_escape(&buffer.join("\n"))
    )
}

pub fn render_block_tex(state: &mut RenderState, buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    let tex_content = buffer.join("\n");
    let svg = get_or_render_latex(state.ctx, &tex_content, false).unwrap_or(tex_content);
    format!("<div class=\"latex\">\n{}\n</div>\n", svg)
}

pub fn render_block_quote(state: &mut RenderState, buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    let mut b_id = "rnd-quote".to_string();
    let mut b_width = String::new();
    let mut b_style = String::new();
    let mut q_cite = String::new();
    let mut q_text = String::new();
    let mut current_key = String::new();
    let mut current_val = String::new();

    let flush = |key: &str,
                 val: &str,
                 b_id: &mut String,
                 b_width: &mut String,
                 b_style: &mut String,
                 q_cite: &mut String,
                 q_text: &mut String| {
        match key {
            "__id__" => *b_id = val.to_string(),
            "__width__" => *b_width = val.to_string(),
            "__style__" => *b_style = val.to_string(),
            "cite" => *q_cite = val.to_string(),
            "text" => {
                if !q_text.is_empty() {
                    q_text.push('\n');
                }
                q_text.push_str(val);
            }
            _ => {}
        }
    };

    for b_line in buffer {
        let l_trim = b_line.trim_start();
        if let Some(after_col) = l_trim.strip_prefix("::") {
            let after_col = after_col.trim_start();
            if let Some(idx) = after_col.find(':') {
                let k_part = &after_col[..idx];
                if !k_part.is_empty()
                    && k_part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut b_id,
                            &mut b_width,
                            &mut b_style,
                            &mut q_cite,
                            &mut q_text,
                        );
                    }
                    current_key = k_part.to_string();
                    current_val = after_col[idx + 1..].trim().to_string();
                    continue;
                }
            }
        }
        if !current_key.is_empty() {
            current_val.push('\n');
            current_val.push_str(b_line);
        } else {
            if !q_text.is_empty() {
                q_text.push('\n');
            }
            q_text.push_str(b_line);
        }
    }
    if !current_key.is_empty() {
        flush(
            &current_key,
            &current_val,
            &mut b_id,
            &mut b_width,
            &mut b_style,
            &mut q_cite,
            &mut q_text,
        );
    }

    let parsed = parse_block_field(state, q_text.trim(), "");
    let style_attr = get_style_attr(&b_width, &b_style);
    let cite_attr = if q_cite.is_empty() {
        String::new()
    } else {
        format!(" cite=\"{}\"", q_cite)
    };
    format!(
        "<blockquote id=\"block-quote-{}\" class=\"block-quote\"{}{}>\n    {}\n</blockquote>\n",
        b_id, style_attr, cite_attr, parsed
    )
}

pub fn render_block_image(state: &mut RenderState, buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    let (_, vals) = parse_kv_buffer(buffer, false);
    let b_id = vals.get("__id__").cloned().unwrap_or_else(|| "rnd-img".to_string());
    let mut b_img = vals.get("image").cloned().unwrap_or_default();
    let b_alt = vals.get("alt").cloned().unwrap_or_default();
    let b_cap = vals.get("__caption__").cloned().unwrap_or_default();
    let b_align = vals.get("align").cloned().unwrap_or_default();
    let b_width = vals.get("__width__").cloned().unwrap_or_default();
    let mut b_style = vals.get("__style__").cloned().unwrap_or_default();
    let i_width = vals.get("width").cloned().unwrap_or_default();
    let i_style = vals.get("style").cloned().unwrap_or_default();

    if !b_img.is_empty()
        && !b_img.starts_with("http://")
        && !b_img.starts_with("https://")
        && !b_img.starts_with('/')
    {
        let mut target_dir = String::new();
        if let Some(stripped) = b_img.strip_prefix("./") {
            let dir_path = if state.current_dir == "." || state.current_dir == "/" {
                String::new()
            } else {
                state.current_dir.replace('.', "/")
            };
            if !dir_path.is_empty() {
                target_dir = format!("/{}", dir_path);
            }
            b_img = stripped.to_string();
        }
        if !b_img.contains('.') {
            let assets_base = state
                .ctx
                .cfg
                .build_dir
                .join(format!("_assets{}", target_dir));
            let mut found_ext = String::new();
            for ext in ["webp", "png", "jpg", "jpeg"] {
                if assets_base.join(format!("{}.{}", b_img, ext)).exists() {
                    found_ext = ext.to_string();
                    break;
                }
            }
            if !found_ext.is_empty() && found_ext != "webp" {
                let webp_target = assets_base.join(format!("{}.webp", b_img));
                if !webp_target.exists() {
                    let src_img = assets_base.join(format!("{}.{}", b_img, found_ext));
                    if Command::new("cwebp")
                        .args(["-q", "80"])
                        .arg(&src_img)
                        .arg("-o")
                        .arg(&webp_target)
                        .output()
                        .map(|o| o.status.success())
                        .unwrap_or(false)
                    {
                        found_ext = "webp".to_string();
                    }
                }
            }
            if !found_ext.is_empty() {
                b_img = format!("{}.{}", b_img, found_ext);
            }
        }
        b_img = format!("/_assets{}/{}", target_dir, b_img);
    }

    if !b_align.is_empty() {
        b_style = format!(
            "display: flex; flex-direction: column; align-items: {}; justify-content: {}; text-align: {}; {}",
            b_align, b_align, b_align, b_style
        );
    }

    let style_attr = get_style_attr(&b_width, &b_style);
    let mut img_style_attr = get_style_attr(&i_width, &i_style);
    if img_style_attr.is_empty() {
        img_style_attr = " style=\"width: inherit;\"".to_string();
    } else if !img_style_attr.contains("width") {
        img_style_attr.pop(); // remove trailing quote
        img_style_attr.push_str(" width: inherit;\"");
    }

    let mut html = format!(
        "<figure id=\"block-image-{}\" class=\"block-image\"{}>\n    <img src=\"{}\" alt=\"{}\" class=\"block-image-img\"{}>\n",
        b_id, style_attr, b_img, b_alt, img_style_attr
    );
    if !b_cap.is_empty() {
        html.push_str(&format!(
            "    <figcaption class=\"block-image-caption\">{}</figcaption>\n",
            b_cap
        ));
    }
    html.push_str("</figure>\n");
    html
}

pub fn render_block_hide(state: &mut RenderState, buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    let (_, vals) = parse_kv_buffer(buffer, true);
    let raw_title = vals.get("title").cloned().unwrap_or_default();
    let raw_hide = vals.get("hide").cloned().unwrap_or_default();
    let b_width = vals.get("__width__").cloned().unwrap_or_default();
    let b_style = vals.get("__style__").cloned().unwrap_or_default();

    let h_hide = parse_block_field(state, &raw_hide, "");
    let h_title = parse_inline_code_only(&html_escape(&raw_title));
    let style_attr = get_style_attr(&b_width, &b_style);

    format!(
        "<details class=\"hide-block\"{}>\n<summary class=\"hide-title\">{}</summary>\n<div class=\"hide-content\">\n{}\n</div>\n</details>\n",
        style_attr, h_title, h_hide
    )
}

pub fn render_block_buttons(_state: &mut RenderState, buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    let mut b_id = "rnd-btns".to_string();
    let mut b_width = String::new();
    let mut b_style = String::new();
    let mut btns: Vec<String> = Vec::new();

    let mut current_key = String::new();
    let mut current_val = String::new();
    let flush = |k: &str,
                 v: &str,
                 b_id: &mut String,
                 b_width: &mut String,
                 b_style: &mut String,
                 btns: &mut Vec<String>| {
        match k {
            "__id__" => *b_id = v.to_string(),
            "button" => btns.push(v.to_string()),
            "__width__" => *b_width = v.to_string(),
            "__style__" => *b_style = v.to_string(),
            _ => {}
        }
    };

    for b_line in buffer {
        let l_trim = b_line.trim_start();
        if let Some(after_col) = l_trim.strip_prefix("::") {
            let after_col = after_col.trim_start();
            if let Some(idx) = after_col.find(':') {
                let k_part = &after_col[..idx];
                if !k_part.is_empty()
                    && k_part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut b_id,
                            &mut b_width,
                            &mut b_style,
                            &mut btns,
                        );
                    }
                    current_key = k_part.to_string();
                    current_val = after_col[idx + 1..].trim().to_string();
                    continue;
                }
            }
        }
        if !current_key.is_empty() {
            current_val.push('\n');
            current_val.push_str(b_line);
        }
    }
    if !current_key.is_empty() {
        flush(
            &current_key,
            &current_val,
            &mut b_id,
            &mut b_width,
            &mut b_style,
            &mut btns,
        );
    }

    let style_attr = get_style_attr(&b_width, &b_style);
    let mut html = format!(
        "<buttons id=\"block-card-buttons-{}\" class=\"block-card-buttons\"{}>\n",
        b_id, style_attr
    );

    for (idx, btn) in btns.iter().enumerate() {
        let mut temp_text = extract_prop_val(btn, "text").unwrap_or_default();
        let mut temp_url = extract_prop_val(btn, "url").unwrap_or_default();

        if temp_url.is_empty() {
            if let Some(p) = extract_prop_val(btn, "path") {
                temp_url = if p.starts_with('/') {
                    p
                } else {
                    format!("/{}", p)
                };
                if let Some(stem) = temp_url.strip_suffix(".md") {
                    temp_url = format!("{}.html", stem);
                } else if !temp_url.contains('.') {
                    temp_url.push_str(".html");
                }
            }
        }
        if temp_text.is_empty() && !temp_url.is_empty() {
            let fname = temp_url.rsplit('/').next().unwrap_or(&temp_url);
            let stem = fname.split('.').next().unwrap_or(fname);
            temp_text = stem.replace(['-', '_'], " ");
        }
        let js_click = if temp_url.starts_with("http://") || temp_url.starts_with("https://") {
            format!("window.open('{}', '_blank');", temp_url)
        } else {
            format!("window.location.href='{}';", temp_url)
        };
        html.push_str(&format!(
            "    <button id=\"block-card-button-{}-{}\" class=\"block-card-button-{}\" href=\"{}\" onclick=\"{}\">{}</button>\n",
            idx + 1,
            b_id,
            idx + 1,
            temp_url,
            js_click,
            temp_text
        ));
    }
    html.push_str("</buttons>\n");
    html
}

fn extract_prop_val(s: &str, key: &str) -> Option<String> {
    let pat = format!("{}=", key);
    let idx = s.find(&pat)?;
    let rest = &s[idx + pat.len()..];
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else if let Some(stripped) = rest.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(stripped[..end].to_string())
    } else {
        let end = rest.find(',').unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

pub fn render_block_card(state: &mut RenderState, buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    let mut b_id = "rnd-card".to_string();
    let mut c_cover = String::new();
    let mut c_text = String::new();
    let mut b_width = String::new();
    let mut b_style = String::new();
    let mut btns: Vec<String> = Vec::new();

    let mut current_key = String::new();
    let mut current_val = String::new();

    let flush = |k: &str,
                 v: &str,
                 b_id: &mut String,
                 c_cover: &mut String,
                 c_text: &mut String,
                 btns: &mut Vec<String>,
                 b_width: &mut String,
                 b_style: &mut String| {
        match k {
            "__id__" => *b_id = v.to_string(),
            "cover" => *c_cover = v.to_string(),
            "text" => *c_text = v.to_string(),
            "button" => btns.push(v.to_string()),
            "__width__" => *b_width = v.to_string(),
            "__style__" => *b_style = v.to_string(),
            _ => {}
        }
    };

    for b_line in buffer {
        let l_trim = b_line.trim_start();
        if let Some(after_col) = l_trim.strip_prefix("::") {
            let after_col = after_col.trim_start();
            if let Some(idx) = after_col.find(':') {
                let k_part = &after_col[..idx];
                if !k_part.is_empty()
                    && k_part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut b_id,
                            &mut c_cover,
                            &mut c_text,
                            &mut btns,
                            &mut b_width,
                            &mut b_style,
                        );
                    }
                    current_key = k_part.to_string();
                    current_val = after_col[idx + 1..].trim().to_string();
                    continue;
                }
            }
        }
        if !current_key.is_empty() {
            current_val.push('\n');
            current_val.push_str(b_line);
        }
    }
    if !current_key.is_empty() {
        flush(
            &current_key,
            &current_val,
            &mut b_id,
            &mut c_cover,
            &mut c_text,
            &mut btns,
            &mut b_width,
            &mut b_style,
        );
    }

    let style_attr = get_style_attr(&b_width, &b_style);
    let mut html = format!(
        "<card id=\"block-card-{}\" class=\"block-card\"{}>\n",
        b_id, style_attr
    );

    if !c_cover.is_empty() {
        let mut cover_src = c_cover;
        if !cover_src.starts_with("http://")
            && !cover_src.starts_with("https://")
            && !cover_src.starts_with('/')
        {
            if !cover_src.contains('.') {
                let base = state.ctx.cfg.build_dir.join("_assets");
                for ext in ["webp", "png", "jpg", "jpeg"] {
                    if base.join(format!("{}.{}", cover_src, ext)).exists() {
                        cover_src = format!("{}.{}", cover_src, ext);
                        break;
                    }
                }
            }
            cover_src = format!("/_assets/{}", cover_src);
        }
        html.push_str(&format!(
            "    <cover id=\"block-card-cover-{}\" class=\"block-card-cover\">\n        <img id=\"block-card-cover-img-{}\" class=\"block-card-cover-img\" src=\"{}\"></img>\n    </cover>\n",
            b_id, b_id, cover_src
        ));
    }

    if !c_text.is_empty() {
        let expanded = c_text.replace("\\n", "\n");
        let parsed = parse_block_field(state, &expanded, "");
        html.push_str(&format!(
            "    <text id=\"block-card-text-{}\" class=\"block-card-text\">{}</text>\n",
            b_id, parsed
        ));
    }

    if !btns.is_empty() {
        let mut fake = vec![format!(":: __id__: {}", b_id)];
        for btn in btns {
            fake.push(format!(":: button: {}", btn));
        }
        html.push_str(&render_block_buttons(state, &fake));
    }
    html.push_str("</card>\n");
    html
}

fn extract_child_id(child_buffer: &[String]) -> Option<String> {
    for l in child_buffer {
        let t = l.trim_start();
        if let Some(after) = t.strip_prefix("::") {
            let after = after.trim_start();
            if let Some(rest) = after.strip_prefix("__id__") {
                let rest = rest.trim_start();
                if let Some(val) = rest.strip_prefix(':') {
                    return Some(val.trim().to_string());
                }
            }
        }
    }
    None
}

fn parse_open_block_line(line: &str) -> Option<(&str, &str)> {
    let trimmed_end = line.trim_end();
    let trimmed_start = trimmed_end.trim_start();
    let indent = &line[..line.len() - line.trim_start().len()];
    if trimmed_start.starts_with("{{") && trimmed_start.ends_with("}}") {
        let inner = trimmed_start[2..trimmed_start.len() - 2].trim();
        if inner == "block.end" {
            return Some((indent, "end"));
        } else if let Some(bname) = inner.strip_prefix("block.") {
            if !bname.is_empty()
                && bname
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Some((indent, bname));
            }
        }
    }
    None
}

pub fn render_block_text(state: &mut RenderState, buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    let mut t_id = "rnd-text".to_string();
    let mut t_text = String::new();
    let mut b_width = String::new();
    let mut b_style = String::new();
    let mut t_width = String::new();
    let mut t_style = String::new();

    let mut child_html: HashMap<String, String> = HashMap::new();
    let mut child_order: Vec<String> = Vec::new();
    let mut in_child = false;
    let mut child_name = String::new();
    let mut child_indent = String::new();
    let mut child_buffer: Vec<String> = Vec::new();

    let mut current_key = String::new();
    let mut current_val = String::new();

    let flush = |k: &str,
                 v: &str,
                 t_id: &mut String,
                 t_text: &mut String,
                 b_width: &mut String,
                 b_style: &mut String,
                 t_width: &mut String,
                 t_style: &mut String| {
        let v_clean = v.trim_start_matches(['\n', '\r']).trim_end();
        match k {
            "__id__" => *t_id = v_clean.to_string(),
            "text" => *t_text = v_clean.to_string(),
            "__width__" => *b_width = v_clean.to_string(),
            "__style__" => *b_style = v_clean.to_string(),
            "width" => *t_width = v_clean.to_string(),
            "style" => *t_style = v_clean.to_string(),
            _ => {}
        }
    };

    for b_line in buffer {
        if in_child {
            if let Some((ind, btype)) = parse_open_block_line(b_line) {
                if btype == "end" && ind == child_indent {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut t_id,
                            &mut t_text,
                            &mut b_width,
                            &mut b_style,
                            &mut t_width,
                            &mut t_style,
                        );
                        current_key.clear();
                    }
                    let cid = extract_child_id(&child_buffer)
                        .unwrap_or_else(|| format!("child-{}", child_order.len()));
                    let rendered = dispatch_block(state, &child_name, &child_buffer);
                    child_html.insert(cid.clone(), rendered);
                    child_order.push(cid);
                    in_child = false;
                    continue;
                }
            }
            child_buffer.push(b_line.clone());
            continue;
        }

        if let Some((ind, btype)) = parse_open_block_line(b_line) {
            if btype != "end" {
                in_child = true;
                child_indent = ind.to_string();
                child_name = btype.to_string();
                child_buffer.clear();
                if !current_key.is_empty() {
                    flush(
                        &current_key,
                        &current_val,
                        &mut t_id,
                        &mut t_text,
                        &mut b_width,
                        &mut b_style,
                        &mut t_width,
                        &mut t_style,
                    );
                    current_key.clear();
                }
                continue;
            }
        }

        let l_trim = b_line.trim_start();
        if let Some(after_col) = l_trim.strip_prefix("::") {
            let after_col = after_col.trim_start();
            if after_col.starts_with("end:") || after_col.starts_with("end ") {
                if !current_key.is_empty() {
                    flush(
                        &current_key,
                        &current_val,
                        &mut t_id,
                        &mut t_text,
                        &mut b_width,
                        &mut b_style,
                        &mut t_width,
                        &mut t_style,
                    );
                    current_key.clear();
                }
                continue;
            }
            if let Some(idx) = after_col.find(':') {
                let k_part = &after_col[..idx];
                if !k_part.is_empty()
                    && k_part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut t_id,
                            &mut t_text,
                            &mut b_width,
                            &mut b_style,
                            &mut t_width,
                            &mut t_style,
                        );
                    }
                    current_key = k_part.to_string();
                    current_val = after_col[idx + 1..].trim().to_string();
                    continue;
                }
            }
        }
        if !current_key.is_empty() {
            current_val.push('\n');
            current_val.push_str(b_line);
        }
    }
    if !current_key.is_empty() {
        flush(
            &current_key,
            &current_val,
            &mut t_id,
            &mut t_text,
            &mut b_width,
            &mut b_style,
            &mut t_width,
            &mut t_style,
        );
    }

    let parsed_text = parse_block_field(state, &t_text, "");
    let style_attr = get_style_attr(&b_width, &b_style);
    let tag_style_attr = get_style_attr(&t_width, &t_style);

    let mut html = format!(
        "<div id=\"block-text-wrapper-{}\" class=\"block-text-wrapper\"{}>\n    <text id=\"block-text-{}\" class=\"block-text\"{}>\n        {}\n",
        t_id, style_attr, t_id, tag_style_attr, parsed_text
    );
    for cid in &child_order {
        if let Some(ch) = child_html.get(cid) {
            html.push_str(ch);
            html.push('\n');
        }
    }
    html.push_str("    </text>\n</div>\n");
    html
}

pub fn render_block_row(state: &mut RenderState, buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    let mut r_id = "rnd-row".to_string();
    let mut r_gap = String::new();
    let mut b_width = String::new();
    let mut b_style = String::new();
    let mut items: Vec<String> = Vec::new();
    let mut child_html: HashMap<String, String> = HashMap::new();

    let mut in_child = false;
    let mut child_name = String::new();
    let mut child_indent = String::new();
    let mut child_buffer: Vec<String> = Vec::new();

    let mut current_key = String::new();
    let mut current_val = String::new();

    let flush = |k: &str,
                 v: &str,
                 r_id: &mut String,
                 r_gap: &mut String,
                 items: &mut Vec<String>,
                 b_width: &mut String,
                 b_style: &mut String| {
        let v_clean = v.trim_start_matches(['\n', '\r']).trim_end();
        match k {
            "__id__" => *r_id = v_clean.to_string(),
            "gap" => *r_gap = v_clean.to_string(),
            "item" => items.push(v_clean.to_string()),
            "__width__" => *b_width = v_clean.to_string(),
            "__style__" => *b_style = v_clean.to_string(),
            _ => {}
        }
    };

    for b_line in buffer {
        if in_child {
            if let Some((ind, btype)) = parse_open_block_line(b_line) {
                if btype == "end" && ind == child_indent {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut r_id,
                            &mut r_gap,
                            &mut items,
                            &mut b_width,
                            &mut b_style,
                        );
                        current_key.clear();
                    }
                    let cid_opt = extract_child_id(&child_buffer);
                    let rendered = dispatch_block(state, &child_name, &child_buffer);
                    if let Some(cid) = cid_opt {
                        child_html.insert(cid, rendered);
                    }
                    in_child = false;
                    continue;
                }
            }
            child_buffer.push(b_line.clone());
            continue;
        }

        if let Some((ind, btype)) = parse_open_block_line(b_line) {
            if btype != "end" {
                in_child = true;
                child_indent = ind.to_string();
                child_name = btype.to_string();
                child_buffer.clear();
                if !current_key.is_empty() {
                    flush(
                        &current_key,
                        &current_val,
                        &mut r_id,
                        &mut r_gap,
                        &mut items,
                        &mut b_width,
                        &mut b_style,
                    );
                    current_key.clear();
                }
                continue;
            }
        }

        let l_trim = b_line.trim_start();
        if let Some(after_col) = l_trim.strip_prefix("::") {
            let after_col = after_col.trim_start();
            if let Some(idx) = after_col.find(':') {
                let k_part = &after_col[..idx];
                if !k_part.is_empty()
                    && k_part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut r_id,
                            &mut r_gap,
                            &mut items,
                            &mut b_width,
                            &mut b_style,
                        );
                    }
                    current_key = k_part.to_string();
                    current_val = after_col[idx + 1..].trim().to_string();
                    continue;
                }
            }
        }
        if !current_key.is_empty() {
            current_val.push('\n');
            current_val.push_str(b_line);
        }
    }
    if !current_key.is_empty() {
        flush(
            &current_key,
            &current_val,
            &mut r_id,
            &mut r_gap,
            &mut items,
            &mut b_width,
            &mut b_style,
        );
    }

    let items_style = if !r_gap.is_empty() {
        let gap_v = if r_gap.bytes().all(|b| b.is_ascii_digit()) {
            format!("{}px", r_gap)
        } else {
            r_gap
        };
        format!(" style=\"gap: {};\"", gap_v)
    } else {
        String::new()
    };

    let style_attr = get_style_attr(&b_width, &b_style);
    let mut html = format!(
        "<row id=\"block-row-{}\" class=\"block-row\"{}>\n    <items id=\"block-row-items-{}\" class=\"block-row-items\"{}>\n",
        r_id, style_attr, r_id, items_style
    );

    for (idx, it) in items.iter().enumerate() {
        let (it_id, it_pos) = parse_id_pos_spec(it);
        let mut class_attr = format!("block-row-item-{}", idx + 1);
        let mut inline_child_style = String::new();
        if !it_pos.is_empty() {
            class_attr.push_str(&format!(" pos-{}", it_pos));
            match it_pos.as_str() {
                "center" => inline_child_style = " style=\"display: flex; flex-direction: column; justify-content: center; align-items: center; text-align: center;\"".to_string(),
                "left" => inline_child_style = " style=\"display: flex; flex-direction: column; justify-content: flex-start; align-items: flex-start; text-align: left;\"".to_string(),
                "right" => inline_child_style = " style=\"display: flex; flex-direction: column; justify-content: flex-end; align-items: flex-end; text-align: right;\"".to_string(),
                _ => {}
            }
        }
        let child_content = child_html.get(&it_id).map(|s| s.as_str()).unwrap_or("");
        html.push_str(&format!(
            "        <item id=\"block-row-item-{}-{}\" class=\"{}\" data-pos=\"{}\"{}>\n{}        </item>\n",
            idx + 1,
            r_id,
            class_attr,
            it_pos,
            inline_child_style,
            child_content
        ));
    }
    html.push_str("    </items>\n</row>\n");
    html
}

fn parse_id_pos_spec(spec: &str) -> (String, String) {
    let mut id = String::new();
    let mut pos = String::new();
    if spec.contains('=') {
        for tok in spec.split(',') {
            let tok = tok.trim();
            if let Some(v) = tok.strip_prefix("id=") {
                id = strip_quotes(v).trim().to_string();
            } else if let Some(v) = tok.strip_prefix("pos=") {
                pos = strip_quotes(v).trim().to_string();
            }
        }
    } else {
        id = strip_quotes(spec).trim().to_string();
    }
    (id, pos)
}

pub fn render_block_carousel(state: &mut RenderState, buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    let mut c_id = "rnd-carousel".to_string();
    let mut c_items = "1".to_string();
    let mut b_width = String::new();
    let mut b_style = String::new();
    let mut items: Vec<String> = Vec::new();
    let mut child_html: HashMap<String, String> = HashMap::new();

    let mut in_child = false;
    let mut child_name = String::new();
    let mut child_buffer: Vec<String> = Vec::new();
    let mut current_key = String::new();
    let mut current_val = String::new();

    let flush = |k: &str,
                 v: &str,
                 c_id: &mut String,
                 c_items: &mut String,
                 items: &mut Vec<String>,
                 b_width: &mut String,
                 b_style: &mut String| {
        let v_clean = v.trim_start_matches(['\n', '\r']).trim_end();
        match k {
            "__id__" => *c_id = v_clean.to_string(),
            "item" => {
                let id_part = v_clean.strip_prefix("id=").unwrap_or(v_clean);
                items.push(id_part.to_string());
            }
            "items" => *c_items = v_clean.to_string(),
            "__width__" => *b_width = v_clean.to_string(),
            "__style__" => *b_style = v_clean.to_string(),
            _ => {}
        }
    };

    for b_line in buffer {
        if in_child {
            if let Some((_, btype)) = parse_open_block_line(b_line) {
                if btype == "end" {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut c_id,
                            &mut c_items,
                            &mut items,
                            &mut b_width,
                            &mut b_style,
                        );
                        current_key.clear();
                    }
                    let cid_opt = extract_child_id(&child_buffer);
                    let rendered = dispatch_block(state, &child_name, &child_buffer);
                    if let Some(cid) = cid_opt {
                        child_html.insert(cid, rendered);
                    }
                    in_child = false;
                    continue;
                }
            }
            child_buffer.push(b_line.clone());
            continue;
        }

        if let Some((_, btype)) = parse_open_block_line(b_line) {
            if btype != "end" {
                in_child = true;
                child_name = btype.to_string();
                child_buffer.clear();
                if !current_key.is_empty() {
                    flush(
                        &current_key,
                        &current_val,
                        &mut c_id,
                        &mut c_items,
                        &mut items,
                        &mut b_width,
                        &mut b_style,
                    );
                    current_key.clear();
                }
                continue;
            }
        }

        let l_trim = b_line.trim_start();
        if let Some(after_col) = l_trim.strip_prefix("::") {
            let after_col = after_col.trim_start();
            if let Some(idx) = after_col.find(':') {
                let k_part = &after_col[..idx];
                if !k_part.is_empty()
                    && k_part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut c_id,
                            &mut c_items,
                            &mut items,
                            &mut b_width,
                            &mut b_style,
                        );
                    }
                    current_key = k_part.to_string();
                    current_val = after_col[idx + 1..].trim().to_string();
                    continue;
                }
            }
        }
        if !current_key.is_empty() {
            current_val.push('\n');
            current_val.push_str(b_line);
        }
    }
    if !current_key.is_empty() {
        flush(
            &current_key,
            &current_val,
            &mut c_id,
            &mut c_items,
            &mut items,
            &mut b_width,
            &mut b_style,
        );
    }

    let raw_c_items = c_items.replace("&lt;", "<");
    let stripped: String = raw_c_items.chars().filter(|c| !c.is_whitespace()).collect();
    let (c_items_num, c_items_attr) = if let Some(num_s) = stripped.strip_prefix("<=") {
        if let Ok(n) = num_s.parse::<usize>() {
            (n.max(1), format!("<={}", n))
        } else {
            (1, "1".to_string())
        }
    } else if let Ok(n) = stripped.parse::<usize>() {
        (n.max(1), n.to_string())
    } else {
        (1, "1".to_string())
    };

    let style_attr = get_style_attr(&b_width, &b_style);
    let mut html = format!(
        "<carousel id=\"block-carousel-{}\" class=\"block-carousel\" data-items=\"{}\"{}>\n    <div class=\"carousel-wrapper\">\n        <button class=\"carousel-btn-prev\">Prev</button>\n        <slides id=\"block-carousel-slides-{}\" class=\"block-carousel-slides\">\n",
        c_id, c_items_attr, style_attr, c_id
    );

    for (idx, it) in items.iter().enumerate() {
        let clean_it = strip_quotes(it.split(',').next().unwrap_or(it)).trim();
        let display_style = if idx + 1 > c_items_num {
            " style=\"display: none;\""
        } else {
            ""
        };
        let child = child_html.get(clean_it).map(|s| s.as_str()).unwrap_or("");
        html.push_str(&format!(
            "            <slide id=\"block-carousel-slide-{}-{}\" class=\"block-carousel-slide\"{}>\n{}            </slide>\n",
            idx + 1,
            c_id,
            display_style,
            child
        ));
    }

    html.push_str("        </slides>\n        <button class=\"carousel-btn-next\">Next</button>\n    </div>\n    <nav class=\"carousel-dots\">\n");
    let dots_count = (items.len() + c_items_num - 1) / c_items_num;
    for j in 0..dots_count {
        let active = if j == 0 { " active" } else { "" };
        html.push_str(&format!(
            "        <button class=\"carousel-dot{}\" data-index=\"{}\"></button>\n",
            active, j
        ));
    }
    html.push_str("    </nav>\n</carousel>\n");
    html
}

pub fn render_block_grid(state: &mut RenderState, buffer: &[String]) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    let mut g_id = "rnd-grid".to_string();
    let mut g_gap = String::new();
    let mut b_width = String::new();
    let mut b_style = String::new();
    let mut grid_rows: Vec<String> = Vec::new();
    let mut grid_child_html: HashMap<String, String> = HashMap::new();

    let mut in_child = false;
    let mut child_name = String::new();
    let mut child_indent = String::new();
    let mut child_buffer: Vec<String> = Vec::new();
    let mut current_key = String::new();
    let mut current_val = String::new();

    let flush = |k: &str,
                 v: &str,
                 g_id: &mut String,
                 g_gap: &mut String,
                 grid_rows: &mut Vec<String>,
                 b_width: &mut String,
                 b_style: &mut String| {
        let v_clean = v.trim_start_matches(['\n', '\r']).trim_end();
        match k {
            "__id__" => *g_id = v_clean.to_string(),
            "gap" => *g_gap = v_clean.to_string(),
            "row" => grid_rows.push(v_clean.to_string()),
            "__width__" => *b_width = v_clean.to_string(),
            "__style__" => *b_style = v_clean.to_string(),
            _ => {}
        }
    };

    for b_line in buffer {
        if in_child {
            if let Some((ind, btype)) = parse_open_block_line(b_line) {
                if btype == "end" && ind == child_indent {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut g_id,
                            &mut g_gap,
                            &mut grid_rows,
                            &mut b_width,
                            &mut b_style,
                        );
                        current_key.clear();
                    }
                    let cid_opt = extract_child_id(&child_buffer);
                    let rendered = dispatch_block(state, &child_name, &child_buffer);
                    if let Some(cid) = cid_opt {
                        grid_child_html.insert(cid, rendered);
                    }
                    in_child = false;
                    continue;
                }
            }
            child_buffer.push(b_line.clone());
            continue;
        }

        if let Some((ind, btype)) = parse_open_block_line(b_line) {
            if btype != "end" {
                in_child = true;
                child_indent = ind.to_string();
                child_name = btype.to_string();
                child_buffer.clear();
                if !current_key.is_empty() {
                    flush(
                        &current_key,
                        &current_val,
                        &mut g_id,
                        &mut g_gap,
                        &mut grid_rows,
                        &mut b_width,
                        &mut b_style,
                    );
                    current_key.clear();
                }
                continue;
            }
        }

        let l_trim = b_line.trim_start();
        if let Some(after_col) = l_trim.strip_prefix("::") {
            let after_col = after_col.trim_start();
            if let Some(idx) = after_col.find(':') {
                let k_part = &after_col[..idx];
                if !k_part.is_empty()
                    && k_part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    if !current_key.is_empty() {
                        flush(
                            &current_key,
                            &current_val,
                            &mut g_id,
                            &mut g_gap,
                            &mut grid_rows,
                            &mut b_width,
                            &mut b_style,
                        );
                    }
                    current_key = k_part.to_string();
                    current_val = after_col[idx + 1..].trim().to_string();
                    continue;
                }
            }
        }
        if !current_key.is_empty() {
            current_val.push('\n');
            current_val.push_str(b_line);
        }
    }
    if !current_key.is_empty() {
        flush(
            &current_key,
            &current_val,
            &mut g_id,
            &mut g_gap,
            &mut grid_rows,
            &mut b_width,
            &mut b_style,
        );
    }

    let rows_style = if !g_gap.is_empty() {
        let gap_v = if g_gap.bytes().all(|b| b.is_ascii_digit()) {
            format!("{}px", g_gap)
        } else {
            g_gap
        };
        format!(" style=\"gap: {};\"", gap_v)
    } else {
        String::new()
    };

    let style_attr = get_style_attr(&b_width, &b_style);
    let mut html = format!(
        "<grid id=\"block-grid-{}\" class=\"block-grid\"{}>\n    <rows id=\"block-grid-rows-{}\" class=\"block-grid-rows\"{}>\n",
        g_id, style_attr, g_id, rows_style
    );

    for r in &grid_rows {
        let (r_id, r_pos) = parse_id_pos_spec(r);
        let mut class_attr = "block-grid-row".to_string();
        let mut inline_child_style = String::new();
        if !r_pos.is_empty() {
            class_attr.push_str(&format!(" pos-{}", r_pos));
            match r_pos.as_str() {
                "center" => inline_child_style = " style=\"display: flex; flex-direction: column; justify-content: center; align-items: center; text-align: center;\"".to_string(),
                "left" => inline_child_style = " style=\"display: flex; flex-direction: column; justify-content: flex-start; align-items: flex-start; text-align: left;\"".to_string(),
                "right" => inline_child_style = " style=\"display: flex; flex-direction: column; justify-content: flex-end; align-items: flex-end; text-align: right;\"".to_string(),
                _ => {}
            }
        }
        let child_content = grid_child_html.get(&r_id).map(|s| s.as_str()).unwrap_or("");
        let target_tag = format!("<row id=\"block-row-{}\" class=\"block-row\">", r_id);
        if child_content.contains(&target_tag) {
            let new_tag = format!(
                "<row id=\"block-grid-row-{}\" class=\"block-row {}\" data-pos=\"{}\"{}>",
                r_id, class_attr, r_pos, inline_child_style
            );
            html.push_str(&child_content.replacen(&target_tag, &new_tag, 1));
        } else {
            html.push_str(&format!(
                "        <row id=\"block-grid-row-{}\" class=\"{}\" data-pos=\"{}\"{}>\n{}        </row>\n",
                r_id, class_attr, r_pos, inline_child_style, child_content
            ));
        }
    }
    html.push_str("    </rows>\n</grid>\n");
    html
}
