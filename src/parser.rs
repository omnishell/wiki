use crate::blocks::{dispatch_block, dispatch_nested_block, render_block_table};
use crate::model::{
    get_or_render_latex, glob_match, html_escape, parse_kv_args, resolve_wiki_ref, slugify,
    split_num_prefix, strip_quotes, RenderState, WikiContext,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub fn parse_inline_code_only(s: &str) -> String {
    if !s.contains('`') {
        return s.to_string();
    }
    let work = s.replace("\\`", "\x1F");
    let mut out = String::with_capacity(work.len() + 16);
    let mut rem = work.as_str();
    while let Some(i1) = rem.find('`') {
        let prefix = &rem[..i1];
        let rest = &rem[i1 + 1..];
        if let Some(i2) = rest.find('`') {
            let code = &rest[..i2];
            out.push_str(prefix);
            out.push_str("<inline>");
            out.push_str(code);
            out.push_str("</inline>");
            rem = &rest[i2 + 1..];
        } else {
            break;
        }
    }
    out.push_str(rem);
    out.replace('\x1F', "`")
}

pub fn parse_inline_markdown(ctx: &WikiContext, input: &str, allow_underscore_strong: bool) -> String {
    if !input.bytes().any(|b| {
        matches!(
            b,
            b'`' | b'*' | b'_' | b'+' | b'{' | b'[' | b'<' | b'=' | b'$'
        )
    }) {
        return input.to_string();
    }

    let mut masked: Vec<String> = Vec::new();
    let mut line = input.to_string();

    if line.contains("<pre") && line.contains("</pre>") {
        let mut tmp = String::new();
        let mut rem = line.as_str();
        while let Some(p1) = rem.find("<pre") {
            let before = &rem[..p1];
            let rest = &rem[p1..];
            if let Some(p2) = rest.find("</pre>") {
                let full_pre = &rest[..p2 + 6];
                if let Some(gt) = full_pre.find('>') {
                    let open_tag = &full_pre[..gt + 1];
                    let inner = &full_pre[gt + 1..full_pre.len() - 6];
                    let idx = masked.len();
                    masked.push(format!("{}{}</pre>", open_tag, html_escape(inner)));
                    tmp.push_str(before);
                    tmp.push_str(&format!("MASK{}", idx));
                    rem = &rest[p2 + 6..];
                    continue;
                }
            }
            break;
        }
        tmp.push_str(rem);
        line = tmp;
    }

    if line.contains('`') {
        let work = line.replace("\\`", "\x1F");
        let mut tmp = String::new();
        let mut rem = work.as_str();
        while let Some(i1) = rem.find('`') {
            let prefix = &rem[..i1];
            let rest = &rem[i1 + 1..];
            if let Some(i2) = rest.find('`') {
                let code = html_escape(&rest[..i2]);
                let idx = masked.len();
                masked.push(format!("<inline>{}</inline>", code));
                tmp.push_str(prefix);
                tmp.push_str(&format!("MASK{}", idx));
                rem = &rest[i2 + 1..];
            } else {
                break;
            }
        }
        tmp.push_str(rem);
        line = tmp.replace('\x1F', "`");
    }

    if line.contains('$') {
        let mut tmp = String::new();
        let mut rem = line.as_str();
        while let Some(i1) = rem.find('$') {
            let prefix = &rem[..i1];
            let rest = &rem[i1 + 1..];
            if let Some(i2) = rest.find('$') {
                let math_code = &rest[..i2];
                if !math_code.is_empty() {
                    let span = get_or_render_latex(ctx, math_code, true)
                        .unwrap_or_else(|| format!("${}$", math_code));
                    let idx = masked.len();
                    masked.push(span);
                    tmp.push_str(prefix);
                    tmp.push_str(&format!("MASK{}", idx));
                    rem = &rest[i2 + 1..];
                    continue;
                }
            }
            break;
        }
        tmp.push_str(rem);
        line = tmp;
    }

    line = replace_paired_delim(&line, "**", "<strong>", "</strong>");
    if allow_underscore_strong {
        line = replace_paired_delim(&line, "__", "<strong>", "</strong>");
    }
    line = replace_paired_delim(&line, "++", "<ins>", "</ins>");

    if line.contains('_') {
        line = replace_emphasis(&line, !allow_underscore_strong);
    }
    if line.contains("{{") && line.contains("}}") {
        line = parse_refs_and_marks(ctx, &line);
    }
    if line.contains('[') && line.contains("](") {
        line = parse_md_links(&line);
    }

    for (i, code) in masked.iter().enumerate() {
        let tok = format!("MASK{}", i);
        line = line.replace(&tok, code);
    }

    line
}

fn replace_paired_delim(s: &str, delim: &str, open_tag: &str, close_tag: &str) -> String {
    if !s.contains(delim) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rem = s;
    let dlen = delim.len();
    while let Some(i1) = rem.find(delim) {
        let prefix = &rem[..i1];
        let rest = &rem[i1 + dlen..];
        if let Some(i2) = rest.find(delim) {
            out.push_str(prefix);
            out.push_str(open_tag);
            out.push_str(&rest[..i2]);
            out.push_str(close_tag);
            rem = &rest[i2 + dlen..];
        } else {
            break;
        }
    }
    out.push_str(rem);
    out
}

fn replace_emphasis(s: &str, simple_field_mode: bool) -> String {
    if simple_field_mode {
        return replace_paired_delim(s, "_", "<em>", "</em>");
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '_' {
            let prev_ok = i == 0
                || chars[i - 1].is_whitespace()
                || chars[i - 1].is_ascii_punctuation();
            if prev_ok && i + 1 < chars.len() && !chars[i + 1].is_whitespace() && chars[i + 1] != '_' {
                let mut close = None;
                for j in (i + 1)..chars.len() {
                    if chars[j] == '_' {
                        let next_ok = j + 1 == chars.len()
                            || chars[j + 1].is_whitespace()
                            || chars[j + 1].is_ascii_punctuation();
                        if next_ok {
                            close = Some(j);
                        }
                        break;
                    }
                }
                if let Some(j) = close {
                    let inner: String = chars[i + 1..j].iter().collect();
                    if !inner.contains('/') && !inner.contains('=') {
                        out.push_str("<em>");
                        out.push_str(&inner);
                        out.push_str("</em>");
                        i = j + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn parse_refs_and_marks(ctx: &WikiContext, s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rem = s;
    while let Some(i1) = rem.find("{{") {
        let pre = &rem[..i1];
        let rest = &rem[i1 + 2..];
        let Some(i2) = rest.find("}}") else {
            out.push_str(rem);
            return out;
        };
        let inner = &rest[..i2];
        rem = &rest[i2 + 2..];

        let clean_in = inner.trim();
        if let Some(mark) = clean_in.strip_prefix('#') {
            if !mark.is_empty()
                && mark
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
            {
                out.push_str(pre);
                out.push_str(&format!(
                    "<span id=\"{}\" class=\"wiki-mark\"></span>",
                    mark
                ));
                continue;
            }
        } else if let Some(anc) = clean_in.strip_prefix('=') {
            if !anc.is_empty()
                && anc
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
            {
                out.push_str(pre);
                out.push_str(&format!("<a href=\"#{}\">{}</a>", anc, anc));
                continue;
            }
        } else if let Some(colon) = clean_in.find(':') {
            let topic = &clean_in[..colon];
            if !topic.is_empty()
                && topic
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                let after = &clean_in[colon + 1..];
                let (raw_ref, label) = if let Some(c2) = after.find(':') {
                    let r = after[..c2].trim();
                    let l = after[c2 + 1..].trim();
                    (r, if l.is_empty() { r } else { l })
                } else {
                    let r = after.trim();
                    (r, r)
                };
                out.push_str(pre);
                if topic == "page" {
                    out.push_str(&format!(
                        "<a href=\"/{}.html\">{}</a>",
                        raw_ref.replace('.', "/"),
                        label
                    ));
                } else {
                    let ref_key = slugify(raw_ref);
                    out.push_str(&resolve_wiki_ref(ctx, topic, &ref_key, label));
                }
                continue;
            }
        }
        out.push_str(pre);
        out.push_str("{{");
        out.push_str(inner);
        out.push_str("}}");
    }
    out.push_str(rem);
    out
}

fn parse_md_links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rem = s;
    while let Some(b_open) = rem.find('[') {
        let after_open = &rem[b_open + 1..];
        let Some(mid) = after_open.find("](") else {
            break;
        };
        let l_text = &after_open[..mid];
        if l_text.contains('[') || l_text.contains(']') {
            out.push_str(&rem[..b_open + 1]);
            rem = after_open;
            continue;
        }
        let after_mid = &after_open[mid + 2..];
        let Some(p_close) = after_mid.find(')') else {
            break;
        };
        let mut l_href = after_mid[..p_close].to_string();
        if !l_href.starts_with("http://") && !l_href.starts_with("https://") {
            l_href = convert_md_ext_to_html(&l_href);
        }
        out.push_str(&rem[..b_open]);
        out.push_str(&format!("<a href=\"{}\">{}</a>", l_href, l_text));
        rem = &after_mid[p_close + 1..];
    }
    out.push_str(rem);
    out
}

fn convert_md_ext_to_html(href: &str) -> String {
    let split_pos = href.find([' ', '\t', '#']).unwrap_or(href.len());
    let base = &href[..split_pos];
    let tail = &href[split_pos..];
    let lower = base.to_ascii_lowercase();
    if lower.ends_with(".md") {
        format!("{}.html{}", &base[..base.len() - 3], tail)
    } else if lower.ends_with(".markdown") {
        format!("{}.html{}", &base[..base.len() - 9], tail)
    } else {
        href.to_string()
    }
}

fn parse_list_marker(line: &str) -> Option<(usize, &str, &str)> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        let marker = &trimmed[..1];
        return Some((indent, marker, rest.trim_start()));
    }
    if let Some(dot) = trimmed.find('.') {
        let num = &trimmed[..dot];
        if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) {
            let after_dot = &trimmed[dot + 1..];
            if after_dot.starts_with(' ') || after_dot.starts_with('\t') {
                return Some((indent, &trimmed[..dot + 1], after_dot.trim_start()));
            }
        }
    }
    None
}

pub fn parse_block_field(state: &mut RenderState, raw: &str, inherited_ctx: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let normalized = raw.replace('\t', "    ").replace('\u{00A0}', " ");
    let trimmed_field = normalized.trim_matches(|c| matches!(c, ' ' | '\n' | '\r'));
    if trimmed_field.is_empty() {
        return String::new();
    }

    if !trimmed_field.contains('\n')
        && !trimmed_field.starts_with("::")
        && !trimmed_field.contains("```")
        && !trimmed_field.contains("$$")
        && !trimmed_field.contains("block.tex")
        && !trimmed_field.contains("block.code")
        && !trimmed_field.contains("block.diagram")
        && !trimmed_field.contains("block.table")
        && parse_list_marker(trimmed_field).is_none()
    {
        return parse_inline_markdown(state.ctx, trimmed_field, false);
    }

    let mut field_lines: Vec<String> = trimmed_field
        .split('\n')
        .map(|s| s.trim_end_matches('\r').to_string())
        .collect();

    let mut base_indent: Option<usize> = None;
    for l in &field_lines {
        let s = l.trim_start();
        if !s.is_empty() {
            let ind = l.len() - s.len();
            base_indent = Some(base_indent.map_or(ind, |b| b.min(ind)));
        }
    }
    if let Some(bi) = base_indent.filter(|&b| b > 0) {
        for l in &mut field_lines {
            if !l.trim().is_empty() && l.len() >= bi {
                *l = l[bi..].to_string();
            }
        }
    }

    let mut out = String::new();
    let mut in_inner = 0u8;
    let mut in_html_pre = false;
    let mut after_block = false;
    let mut first_code_line = false;
    let mut tex_lines: Vec<String> = Vec::new();
    let mut inner_block_lines: Vec<String> = Vec::new();

    let mut list_indents: Vec<usize> = Vec::new();
    let mut list_types: Vec<&str> = Vec::new();

    let close_all_lists =
        |out: &mut String, indents: &mut Vec<usize>, types: &mut Vec<&str>| {
            while let Some(lt) = types.pop() {
                indents.pop();
                out.push_str(&format!("</li></{}>", lt));
            }
        };

    for orig_l in field_lines {
        let mut l = orig_l.clone();
        if in_inner == 0 {
            let mut l_trim = l.trim_start().to_string();
            if let Some(after) = l_trim.strip_prefix("::") {
                let stripped = after
                    .strip_prefix(' ')
                    .or_else(|| after.strip_prefix('\t'))
                    .unwrap_or(after);
                l = stripped.to_string();
                l_trim = l.trim_start().to_string();
            }
            let r_trim = l_trim.trim_end();

            if let (Some(d_start), Some(d_end)) = (
                r_trim
                    .find("{{block.diagram}}")
                    .or_else(|| r_trim.find("{{ block.diagram }}")),
                r_trim.rfind("{{"),
            ) {
                let end_inner = r_trim[d_end..].trim_start_matches("{{");
                if end_inner.trim_end_matches("}}").trim() == "block.end" && d_end > d_start {
                    let open_end = r_trim[d_start..].find("}}").unwrap() + d_start + 2;
                    let pre_diag = &r_trim[..d_start];
                    let mid_diag = &r_trim[open_end..d_end];
                    let close_end = r_trim[d_end..].find("}}").unwrap() + d_end + 2;
                    let post_diag = &r_trim[close_end..];

                    close_all_lists(&mut out, &mut list_indents, &mut list_types);
                    while out.ends_with('\n') {
                        out.pop();
                    }
                    if !pre_diag.trim().is_empty() {
                        let p = parse_block_field(state, pre_diag, inherited_ctx);
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(&p);
                    }
                    let diag_lines = vec![format!(" {}", mid_diag)];
                    let diag_html =
                        dispatch_nested_block(state, "diagram", &diag_lines, inherited_ctx);
                    out.push_str(diag_html.trim_end_matches('\n'));
                    if !post_diag.trim().is_empty() {
                        out.push_str(&parse_block_field(state, post_diag, inherited_ctx));
                    }
                    after_block = true;
                    continue;
                }
            }

            if r_trim.starts_with("{{") && r_trim.ends_with("}}") {
                let inner_b = r_trim[2..r_trim.len() - 2].trim();
                if inner_b == "block.diagram" {
                    close_all_lists(&mut out, &mut list_indents, &mut list_types);
                    while out.ends_with('\n') {
                        out.pop();
                    }
                    in_inner = 5;
                    inner_block_lines.clear();
                    continue;
                } else if inner_b == "block.table" {
                    close_all_lists(&mut out, &mut list_indents, &mut list_types);
                    while out.ends_with('\n') {
                        out.pop();
                    }
                    in_inner = 6;
                    inner_block_lines.clear();
                    continue;
                } else if inner_b == "block.tex" {
                    close_all_lists(&mut out, &mut list_indents, &mut list_types);
                    while out.ends_with('\n') {
                        out.pop();
                    }
                    in_inner = 3;
                    tex_lines.clear();
                    continue;
                } else if let Some(args) = inner_b.strip_prefix("block.code") {
                    if args.is_empty() || args.starts_with(char::is_whitespace) {
                        close_all_lists(&mut out, &mut list_indents, &mut list_types);
                        while out.ends_with('\n') {
                            out.pop();
                        }
                        in_inner = 2;
                        first_code_line = true;
                        let classes = build_code_classes(args);
                        out.push_str(&format!("<pre><code class=\"{}\">", classes));
                        continue;
                    }
                }
            }

            if r_trim.starts_with("$$") {
                close_all_lists(&mut out, &mut list_indents, &mut list_types);
                while out.ends_with('\n') {
                    out.pop();
                }
                if r_trim == "$$" {
                    in_inner = 4;
                    tex_lines.clear();
                    continue;
                } else if r_trim.ends_with("$$") && r_trim.len() > 4 {
                    let tex_c = &r_trim[2..r_trim.len() - 2];
                    let svg = get_or_render_latex(state.ctx, tex_c, false)
                        .unwrap_or_else(|| tex_c.to_string());
                    out.push_str(&format!("<div class=\"latex\">{}</div>", svg));
                    after_block = true;
                    continue;
                }
            }

            if let Some(fence_rest) = l_trim.strip_prefix("```") {
                let fence_lang = fence_rest.trim_end();
                if fence_lang
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
                {
                    close_all_lists(&mut out, &mut list_indents, &mut list_types);
                    while out.ends_with('\n') {
                        out.pop();
                    }
                    in_inner = 1;
                    first_code_line = true;
                    let mut classes = "hljs".to_string();
                    if !fence_lang.is_empty() && fence_lang != "plaintext" && fence_lang != "none" {
                        classes.push_str(&format!(" language-{}", fence_lang));
                    } else {
                        classes.push_str(" nohighlight");
                    }
                    out.push_str(&format!("<pre><code class=\"{}\">", classes));
                    continue;
                }
            }

            if in_html_pre {
                if l.contains("</pre>") {
                    in_html_pre = false;
                    after_block = true;
                }
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&l);
            } else if l.contains("<pre") && !l.contains("</pre>") {
                while out.ends_with('\n') {
                    out.pop();
                }
                in_html_pre = true;
                out.push_str(&l);
            } else {
                let list_match =
                    parse_list_marker(&l).map(|(i, m, rest)| (i, m.to_string(), rest.to_string()));
                if list_match.is_none() && !list_types.is_empty() {
                    if l_trim.is_empty() {
                        continue;
                    }
                    if l == l_trim {
                        close_all_lists(&mut out, &mut list_indents, &mut list_types);
                        after_block = true;
                    }
                }
                if list_match.is_none() && after_block && l_trim.is_empty() {
                    continue;
                }

                if let Some((indent, marker, item_body)) = list_match {
                    after_block = false;
                    let parsed_item = parse_inline_markdown(state.ctx, &item_body, false);
                    let (ltype, li_tag) = if marker.as_bytes()[0].is_ascii_digit() {
                        ("ol", format!("<li value=\"{}\">", marker.trim_end_matches('.')))
                    } else {
                        ("ul", "<li>".to_string())
                    };
                    if list_types.is_empty() {
                        while out.ends_with('\n') {
                            out.pop();
                        }
                        list_indents.push(indent);
                        list_types.push(ltype);
                        out.push_str(&format!("<{}>{}{}", ltype, li_tag, parsed_item));
                    } else if indent > *list_indents.last().unwrap() {
                        list_indents.push(indent);
                        list_types.push(ltype);
                        out.push_str(&format!("<{}>{}{}", ltype, li_tag, parsed_item));
                    } else {
                        while !list_types.is_empty() && indent < *list_indents.last().unwrap() {
                            let lt = list_types.pop().unwrap();
                            list_indents.pop();
                            out.push_str(&format!("</li></{}>", lt));
                        }
                        if list_types.is_empty() {
                            list_indents.push(indent);
                            list_types.push(ltype);
                            out.push_str(&format!("<{}>{}{}", ltype, li_tag, parsed_item));
                        } else if *list_types.last().unwrap() != ltype {
                            let old_t = list_types.pop().unwrap();
                            list_types.push(ltype);
                            out.push_str(&format!(
                                "</li></{}><{}>{}{}",
                                old_t, ltype, li_tag, parsed_item
                            ));
                        } else {
                            out.push_str(&format!("</li>{}{}", li_tag, parsed_item));
                        }
                    }
                } else {
                    let parsed_line = parse_inline_markdown(state.ctx, &l, false);
                    if after_block {
                        out.push_str(&parsed_line);
                        after_block = false;
                    } else {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(&parsed_line);
                    }
                }
            }
        } else if in_inner == 1 {
            let mut body = l.as_str();
            let lt = l.trim_start();
            if let Some(after) = lt.strip_prefix("::") {
                body = after
                    .strip_prefix(' ')
                    .or_else(|| after.strip_prefix('\t'))
                    .unwrap_or(after);
            }
            if body.trim() == "```" {
                in_inner = 0;
                out.push_str("</code></pre>");
                after_block = true;
            } else {
                if !first_code_line {
                    out.push('\n');
                }
                first_code_line = false;
                out.push_str(&html_escape(body));
            }
        } else if in_inner == 2 {
            let mut body = l.as_str();
            let lt = l.trim_start();
            if let Some(after) = lt.strip_prefix("::") {
                body = after
                    .strip_prefix(' ')
                    .or_else(|| after.strip_prefix('\t'))
                    .unwrap_or(after);
            }
            let rt = body.trim();
            if rt.starts_with("{{") && rt.ends_with("}}") && rt[2..rt.len() - 2].trim() == "block.end"
            {
                in_inner = 0;
                out.push_str("</code></pre>");
                after_block = true;
            } else {
                if !first_code_line {
                    out.push('\n');
                }
                first_code_line = false;
                out.push_str(&html_escape(body));
            }
        } else if in_inner == 3 {
            let mut body = l.as_str();
            let lt = l.trim_start();
            if let Some(after) = lt.strip_prefix("::") {
                body = after
                    .strip_prefix(' ')
                    .or_else(|| after.strip_prefix('\t'))
                    .unwrap_or(after);
            }
            let rt = body.trim();
            if rt.starts_with("{{") && rt.ends_with("}}") && rt[2..rt.len() - 2].trim() == "block.end"
            {
                in_inner = 0;
                let tex_c = tex_lines.join("\n");
                let svg = get_or_render_latex(state.ctx, &tex_c, false).unwrap_or(tex_c);
                out.push_str(&format!("<div class=\"latex\">{}</div>", svg));
                after_block = true;
            } else {
                tex_lines.push(body.to_string());
            }
        } else if in_inner == 4 {
            let mut body = l.as_str();
            let lt = l.trim_start();
            if let Some(after) = lt.strip_prefix("::") {
                body = after
                    .strip_prefix(' ')
                    .or_else(|| after.strip_prefix('\t'))
                    .unwrap_or(after);
            }
            if body.trim() == "$$" {
                in_inner = 0;
                let tex_c = tex_lines.join("\n");
                let svg = get_or_render_latex(state.ctx, &tex_c, false).unwrap_or(tex_c);
                out.push_str(&format!("<div class=\"latex\">{}</div>", svg));
                after_block = true;
            } else {
                tex_lines.push(body.to_string());
            }
        } else if in_inner == 5 {
            let mut check = orig_l.trim_start();
            if let Some(after) = check.strip_prefix("::") {
                check = after.trim_start();
            }
            let rt = check.trim_end();
            if rt.starts_with("{{") && rt.ends_with("}}") && rt[2..rt.len() - 2].trim() == "block.end"
            {
                in_inner = 0;
                let diag_html =
                    dispatch_nested_block(state, "diagram", &inner_block_lines, inherited_ctx);
                out.push_str(diag_html.trim_end_matches('\n'));
                after_block = true;
            } else {
                inner_block_lines.push(orig_l);
            }
        } else if in_inner == 6 {
            let mut body = orig_l.as_str();
            let lt = orig_l.trim_start();
            if let Some(after) = lt.strip_prefix("::") {
                body = after
                    .strip_prefix(' ')
                    .or_else(|| after.strip_prefix('\t'))
                    .unwrap_or(after);
            }
            let rt = body.trim();
            if rt.starts_with("{{") && rt.ends_with("}}") && rt[2..rt.len() - 2].trim() == "block.end"
            {
                in_inner = 0;
                let tbl_html = render_block_table(state, &inner_block_lines);
                out.push_str(tbl_html.trim_end_matches('\n'));
                after_block = true;
            } else {
                inner_block_lines.push(body.to_string());
            }
        }
    }

    close_all_lists(&mut out, &mut list_indents, &mut list_types);
    out
}

fn build_code_classes(args: &str) -> String {
    let mut lang = String::new();
    if let Some(idx) = args.find("lang=") {
        let rest = args[idx + 5..].trim_start_matches(['"', '\'']);
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '_' | '+' | '-' | '#'))
            .unwrap_or(rest.len());
        lang = rest[..end].to_string();
    }
    let mut classes = "hljs".to_string();
    if !lang.is_empty() && lang != "plaintext" && lang != "none" {
        classes.push_str(&format!(" language-{}", lang));
    } else {
        classes.push_str(" nohighlight");
    }
    if args.contains("numbered=true") {
        classes.push_str(" line-numbers");
    }
    classes
}

pub fn parse_page_body(state: &mut RenderState, raw_content: &str) -> String {
    let mut lines: Vec<&str> = raw_content.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }

    let mut start_idx = 0usize;
    if let Some(first) = lines.first() {
        if first.trim_end_matches('\r') == "---" {
            for (idx, l) in lines.iter().enumerate().skip(1) {
                if l.trim_end_matches('\r') == "---" {
                    start_idx = idx + 1;
                    break;
                }
            }
        }
    }

    let mut content_parts: Vec<String> = Vec::with_capacity(lines.len());
    let mut in_code_block = false;
    let mut in_html_pre = false;
    let mut in_custom_block = String::new();
    let mut custom_block_indent = String::new();
    let mut custom_block_depth = 0usize;
    let mut custom_block_buffer: Vec<String> = Vec::new();

    let mut list_indents: Vec<usize> = Vec::new();
    let mut list_types: Vec<&str> = Vec::new();

    let mut section_count = 0usize;
    let mut section_stack: Vec<usize> = Vec::new();

    let mut was_empty = false;
    let mut ignore_next_empty = false;

    let close_lists =
        |parts: &mut Vec<String>, indents: &mut Vec<usize>, types: &mut Vec<&str>| {
            if !types.is_empty() {
                let mut s = String::new();
                while let Some(lt) = types.pop() {
                    indents.pop();
                    s.push_str(&format!("</li></{}>\n", lt));
                }
                parts.push(s);
            }
        };

    for &raw_line in &lines[start_idx..] {
        let mut line = raw_line.trim_end_matches('\r').to_string();
        let mut set_ignore_this_turn = false;

        if in_code_block {
            let lt = line.trim();
            if lt.starts_with("{{") && lt.ends_with("}}") && lt[2..lt.len() - 2].trim() == "block.end"
            {
                content_parts.push("</code></pre>\n".to_string());
                in_code_block = false;
                ignore_next_empty = true;
                was_empty = true;
            } else {
                content_parts.push(format!("{}\n", html_escape(&line)));
            }
            continue;
        }

        if in_custom_block.is_empty() {
            let lt = line.trim();
            if lt.starts_with("{{") && lt.ends_with("}}") {
                let inner = lt[2..lt.len() - 2].trim();
                if let Some(args) = inner.strip_prefix("block.code") {
                    if args.is_empty() || args.starts_with(char::is_whitespace) {
                        close_lists(&mut content_parts, &mut list_indents, &mut list_types);
                        if content_parts.last().map(|s| s.as_str()) == Some("<br><br>\n") {
                            content_parts.pop();
                        }
                        in_code_block = true;
                        let classes = build_code_classes(args);
                        if classes.contains("language-") {
                            content_parts.push(format!("<pre><code class=\"{}\">\n", classes));
                        } else {
                            content_parts.push(format!("<pre><code class=\"{}\">", classes));
                        }
                        continue;
                    }
                }
            }
        }

        let l_trim = line.trim_start();
        let curr_indent = &line[..line.len() - l_trim.len()];
        if !in_custom_block.is_empty() {
            let mut check_trim = l_trim;
            if let Some(after_c) = check_trim.strip_prefix("::") {
                let after_c = after_c.trim_start();
                if after_c.starts_with("{{") && after_c.contains("}}") {
                    check_trim = after_c;
                }
            }
            let r_trim = check_trim.trim_end();
            if r_trim.starts_with("{{") && r_trim.ends_with("}}") {
                let inner = r_trim[2..r_trim.len() - 2].trim();
                if inner == "block.end" {
                    if custom_block_depth > 0 {
                        custom_block_depth -= 1;
                    } else if curr_indent == custom_block_indent {
                        let rendered =
                            dispatch_block(state, &in_custom_block, &custom_block_buffer);
                        if !rendered.is_empty() {
                            content_parts.push(rendered);
                        }
                        in_custom_block.clear();
                        custom_block_indent.clear();
                        custom_block_depth = 0;
                        custom_block_buffer.clear();
                        ignore_next_empty = true;
                        was_empty = true;
                        continue;
                    }
                } else if inner.starts_with("block.")
                    && !r_trim.contains("block.end")
                    && !matches!(
                        in_custom_block.as_str(),
                        "row" | "carousel" | "grid" | "text"
                    )
                {
                    custom_block_depth += 1;
                }
            }

            if in_custom_block == "globals" {
                if let Some(after_col) = l_trim.strip_prefix("::") {
                    let after_col = after_col.trim_start();
                    if let Some(idx) = after_col.find(':') {
                        let gk = after_col[..idx].trim();
                        let mut gv = after_col[idx + 1..].trim().to_string();
                        if gk == "thumb" && gv.starts_with("./") {
                            let dir_path = if state.current_dir == "." || state.current_dir == "/" {
                                String::new()
                            } else {
                                state.current_dir.replace('.', "/")
                            };
                            let rel = &gv[2..];
                            let dot = rel.rfind('.').unwrap_or(rel.len());
                            let webp = format!("{}.webp", &rel[..dot]);
                            gv = if dir_path.is_empty() {
                                format!("/_assets/{}", webp)
                            } else {
                                format!("/_assets/{}/{}", dir_path, webp)
                            };
                        }
                        state.page_vars.insert(format!("page.{}", gk), gv);
                    }
                }
            } else if in_custom_block != "comment" {
                custom_block_buffer.push(line);
            }
            continue;
        } else {
            let r_trim = l_trim.trim_end();
            if r_trim.starts_with("{{") && r_trim.ends_with("}}") {
                let inner = r_trim[2..r_trim.len() - 2].trim();
                if let Some(bname) = inner.strip_prefix("block.") {
                    if !bname.is_empty()
                        && bname
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                    {
                        close_lists(&mut content_parts, &mut list_indents, &mut list_types);
                        if content_parts.last().map(|s| s.as_str()) == Some("<br><br>\n") {
                            content_parts.pop();
                        }
                        in_custom_block = bname.to_string();
                        custom_block_indent = curr_indent.to_string();
                        custom_block_depth = 0;
                        custom_block_buffer.clear();
                        continue;
                    }
                }
            }
        }

        if in_html_pre {
            if let Some(close_idx) = line.find("</pre>") {
                in_html_pre = false;
                let before = html_escape(&line[..close_idx]);
                let after = &line[close_idx + 6..];
                content_parts.push(format!("{}</pre>{}\n", before, after));
            } else {
                content_parts.push(format!("{}\n", html_escape(&line)));
            }
            continue;
        }
        if line.contains("<pre") && !line.contains("</pre>") {
            in_html_pre = true;
            if let Some(gt) = line.find('>') {
                let open_tag = &line[..gt + 1];
                let after = html_escape(&line[gt + 1..]);
                content_parts.push(format!("{}{}\n", open_tag, after));
                continue;
            }
        }

        if line.contains("{{") {
            let trimmed_all = line.trim();
            if trimmed_all.starts_with("{{")
                && trimmed_all.ends_with("}}")
                && trimmed_all[2..trimmed_all.len() - 2].trim() == "toc"
            {
                line = "{{TOC_PLACEHOLDER}}".to_string();
                set_ignore_this_turn = true;
            } else {
                if expand_listings(state, &mut line) {
                    set_ignore_this_turn = true;
                }
                expand_countings(state, &mut line);
            }
        }

        if line.starts_with('#') {
            if let Some(rendered_h) = parse_header_line(
                state,
                &line,
                &mut content_parts,
                &mut list_indents,
                &mut list_types,
            ) {
                line = rendered_h;
                set_ignore_this_turn = true;
            }
        }

        if line.starts_with('@') {
            if let Some(sec_html) = parse_section_line(
                &line,
                &mut section_count,
                &mut section_stack,
                &mut content_parts,
                &mut list_indents,
                &mut list_types,
            ) {
                line = sec_html;
                set_ignore_this_turn = true;
            }
        }

        if let Some((indent, marker, item_body)) =
            parse_list_marker(&line).map(|(i, m, r)| (i, m.to_string(), r.to_string()))
        {
            let ltype = if marker.as_bytes()[0].is_ascii_digit() {
                "ol"
            } else {
                "ul"
            };
            let mut prefix = String::new();
            while !list_types.is_empty() && indent < *list_indents.last().unwrap() {
                let lt = list_types.pop().unwrap();
                list_indents.pop();
                prefix.push_str(&format!("</li></{}>", lt));
            }
            if list_types.is_empty() || indent > *list_indents.last().unwrap() {
                list_indents.push(indent);
                list_types.push(ltype);
                prefix.push_str(&format!("<{}><li>", ltype));
            } else if *list_types.last().unwrap() != ltype {
                let old_t = list_types.pop().unwrap();
                list_types.push(ltype);
                prefix.push_str(&format!("</li></{}><{}><li>", old_t, ltype));
            } else {
                prefix.push_str("</li><li>");
            }
            line = format!("{}{}", prefix, item_body);
        } else if !list_types.is_empty() {
            if line.trim().is_empty() {
                if !ignore_next_empty && !was_empty {
                    content_parts.push("<br><br>\n".to_string());
                    was_empty = true;
                }
                continue;
            }
            if !line.starts_with(char::is_whitespace) {
                close_lists(&mut content_parts, &mut list_indents, &mut list_types);
            }
        }

        line = parse_inline_markdown(state.ctx, &line, true);

        if line.trim().is_empty() {
            if !ignore_next_empty && !was_empty {
                content_parts.push("<br><br>\n".to_string());
                was_empty = true;
            }
        } else {
            content_parts.push(format!("{}\n", line));
            was_empty = false;
            ignore_next_empty = set_ignore_this_turn;
        }
    }

    close_lists(&mut content_parts, &mut list_indents, &mut list_types);
    while section_stack.pop().is_some() {
        content_parts.push("</section>\n".to_string());
    }

    let mut content = content_parts.concat();
    let full_toc = format!("<ul class=\"toc\">\n{}</ul>", state.toc_html);
    content = content.replace("{{TOC_PLACEHOLDER}}", &full_toc);
    content
}

fn parse_header_line(
    state: &mut RenderState,
    line: &str,
    content_parts: &mut Vec<String>,
    list_indents: &mut Vec<usize>,
    list_types: &mut Vec<&str>,
) -> Option<String> {
    if let Some(rest) = line.strip_prefix("#!") {
        let (attr_block, after_attr) = extract_brace_attrs(rest);
        let h_text = after_attr.strip_prefix(' ')?.trim_start();
        let style_str = format_header_attrs(attr_block);
        return Some(format!("<h0{}>{}</h0>", style_str, h_text));
    }

    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    let (attr_block, after_attr) = extract_brace_attrs(rest);
    if !after_attr.starts_with(char::is_whitespace) {
        return None;
    }
    while let Some(lt) = list_types.pop() {
        list_indents.pop();
        content_parts.push(format!("</li></{}>\n", lt));
    }
    let h_text = after_attr.trim_start();
    let style_str = format_header_attrs(attr_block);
    let slug: String = h_text
        .to_lowercase()
        .replace(' ', "_")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    state.toc_html.push_str(&format!(
        "<li class=\"toc-h{}\"><a href=\"#{}\">{}</a></li>\n",
        hashes, slug, h_text
    ));
    Some(format!(
        "<h{} id=\"{}\"{}>{}</h{}>",
        hashes, slug, style_str, h_text, hashes
    ))
}

fn extract_brace_attrs(s: &str) -> (&str, &str) {
    if let Some(stripped) = s.strip_prefix('{') {
        if let Some(close) = stripped.find('}') {
            return (&stripped[..close], &stripped[close + 1..]);
        }
    }
    ("", s)
}

fn format_header_attrs(attr_block: &str) -> String {
    if attr_block.is_empty() {
        return String::new();
    }
    let mut style = String::new();
    for attr in attr_block.split(',') {
        let attr = attr.trim();
        if let Some((k, v)) = attr.split_once('=') {
            let prop = if k == "align" { "text-align" } else { k };
            style.push_str(&format!("{}: {}; ", prop, v));
        }
    }
    let trimmed = style.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(" style=\"{}\"", trimmed)
    }
}

fn parse_section_line(
    line: &str,
    section_count: &mut usize,
    section_stack: &mut Vec<usize>,
    content_parts: &mut Vec<String>,
    list_indents: &mut Vec<usize>,
    list_types: &mut Vec<&str>,
) -> Option<String> {
    let s_level = line.bytes().take_while(|&b| b == b'@').count();
    if s_level == 0 {
        return None;
    }
    while let Some(lt) = list_types.pop() {
        list_indents.pop();
        content_parts.push(format!("</li></{}>\n", lt));
    }
    let rest = &line[s_level..];
    let (attr_block, _) = extract_brace_attrs(rest);
    let mut style = String::new();
    if !attr_block.is_empty() {
        for attr in attr_block.split(',') {
            let attr = attr.trim();
            if let Some((k, v)) = attr.split_once('=') {
                if k == "align" {
                    let flex_align = match v {
                        "left" => "flex-start",
                        "right" => "flex-end",
                        _ => v,
                    };
                    style.push_str(&format!(
                        "display: flex; justify-content: center; flex-direction: column; align-items: {}; text-align: {}; ",
                        flex_align, v
                    ));
                } else {
                    let prop = if k == "bg" { "background-color" } else { k };
                    style.push_str(&format!("{}: {}; ", prop, v));
                }
            }
        }
    }
    let style_str = if style.trim().is_empty() {
        String::new()
    } else {
        format!(" style=\"{}\"", style.trim_end())
    };
    *section_count += 1;
    let mut sec_tag = String::new();
    while let Some(&last) = section_stack.last() {
        if last >= s_level {
            sec_tag.push_str("</section>\n");
            section_stack.pop();
        } else {
            break;
        }
    }
    section_stack.push(s_level);
    Some(format!(
        "{}<section class=\"section-{} section-level-{}\"{}>",
        sec_tag, *section_count, s_level, style_str
    ))
}

fn expand_listings(state: &mut RenderState, line: &mut String) -> bool {
    let mut matched = false;
    loop {
        let Some((start, end, inner)) = find_tag_with_prefix(line, "list.") else {
            break;
        };
        let rest = &inner[5..];
        let replacement = if rest == "pages"
            || rest.starts_with(char::is_whitespace)
            || rest.starts_with("pages ")
            || rest.starts_with("pages\t")
        {
            let args = rest.strip_prefix("pages").unwrap_or("").trim();
            listing_pages(state.ctx, &state.current_dir, args)
        } else if let Some(b_rest) = rest.strip_prefix("block.") {
            let mut sp = b_rest.splitn(2, char::is_whitespace);
            let b_type = sp.next().unwrap_or("");
            let args = sp.next().unwrap_or("").trim();
            listing_blocks(state.ctx, &state.file_path, b_type, args)
        } else {
            let (l_type, l_path) = rest.split_once(':').unwrap_or((rest.trim(), ""));
            listing_blocks(state.ctx, &state.file_path, l_type.trim(), l_path.trim())
        };
        line.replace_range(start..end, &replacement);
        matched = true;
    }
    matched
}

fn expand_countings(state: &mut RenderState, line: &mut String) {
    loop {
        let Some((start, end, inner)) = find_tag_with_prefix(line, "count.") else {
            break;
        };
        let rest = &inner[6..];
        let count_val =
            if rest == "pages" || rest.starts_with("pages ") || rest.starts_with("pages\t") {
                let args = rest.strip_prefix("pages").unwrap_or("").trim();
                counting_pages(state.ctx, &state.current_dir, args)
            } else if let Some(b_rest) = rest.strip_prefix("block.") {
                let mut sp = b_rest.splitn(2, char::is_whitespace);
                let b_type = sp.next().unwrap_or("");
                let args = sp.next().unwrap_or("").trim();
                counting_blocks(state.ctx, &state.file_path, b_type, args)
            } else {
                let (c_type, c_path) = rest.split_once(':').unwrap_or((rest.trim(), ""));
                counting_blocks(state.ctx, &state.file_path, c_type.trim(), c_path.trim())
            };
        line.replace_range(start..end, &count_val.to_string());
    }
}

fn find_tag_with_prefix(s: &str, prefix: &str) -> Option<(usize, usize, String)> {
    let mut offset = 0;
    while let Some(i1) = s[offset..].find("{{") {
        let abs_start = offset + i1;
        let rest = &s[abs_start + 2..];
        let Some(i2) = rest.find("}}") else {
            return None;
        };
        let inner = rest[..i2].trim();
        if inner.starts_with(prefix) {
            return Some((abs_start, abs_start + 2 + i2 + 2, inner.to_string()));
        }
        offset = abs_start + 2;
    }
    None
}

fn match_exclude_expr(
    expr: &str,
    slug_key: &str,
    fname: &str,
    stem: &str,
    fname_no_ext: &str,
) -> bool {
    if expr.is_empty() {
        return false;
    }
    if expr.contains("&&") {
        for part in expr.split('&') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            let m = glob_match(p, slug_key)
                || glob_match(p, fname)
                || glob_match(p, stem)
                || glob_match(p, fname_no_ext);
            if !m {
                return false;
            }
        }
        true
    } else {
        for part in expr.split('|') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            if glob_match(p, slug_key)
                || glob_match(p, fname)
                || glob_match(p, stem)
                || glob_match(p, fname_no_ext)
            {
                return true;
            }
        }
        false
    }
}

fn match_global_req(actual: &str, req_str: &str) -> bool {
    if actual.is_empty() {
        return false;
    }
    if req_str.contains("&&") {
        for r in req_str.split('&') {
            let p = r.trim();
            if !p.is_empty() && !glob_match(p, actual) {
                return false;
            }
        }
        true
    } else {
        for r in req_str.split('|') {
            let p = r.trim();
            if !p.is_empty() && glob_match(p, actual) {
                return true;
            }
        }
        false
    }
}

pub fn listing_pages(ctx: &WikiContext, current_dir: &str, args: &str) -> String {
    let mut l_root = current_dir.to_string();
    let mut l_pattern = "*".to_string();
    let mut l_page_label = "title".to_string();
    let mut l_exclude = String::new();
    let mut l_depth = 0usize;
    let mut l_globals: HashMap<String, String> = HashMap::new();

    for (key, val) in parse_kv_args(args) {
        match key.as_str() {
            "root" => l_root = val,
            "pattern" => l_pattern = val,
            "page_label" => l_page_label = val,
            "exclude" => l_exclude = val,
            "depth" => l_depth = val.parse::<usize>().unwrap_or(0),
            other => {
                let gk = other.strip_prefix("globals.").unwrap_or(other);
                l_globals.insert(gk.to_string(), val);
            }
        }
    }
    if l_root == "." || l_root == "/" {
        l_root.clear();
    }

    let mut clean_l_root_slug = String::new();
    let mut clean_l_root_url = String::new();
    if !l_root.is_empty() {
        let raw_root = l_root.trim_start_matches('/');
        for p_raw in raw_root.split('/') {
            if p_raw.is_empty() {
                continue;
            }
            let p = split_num_prefix(p_raw).map(|(_, r)| r).unwrap_or(p_raw);
            let slug_p = p.replace('/', ".");
            if clean_l_root_slug.is_empty() {
                clean_l_root_slug = slug_p;
            } else {
                clean_l_root_slug.push('.');
                clean_l_root_slug.push_str(&slug_p);
            }
            let u_part = p.replace('.', "/");
            if clean_l_root_url.is_empty() {
                clean_l_root_url = u_part;
            } else {
                clean_l_root_url.push('/');
                clean_l_root_url.push_str(&u_part);
            }
        }
        if l_root.starts_with('/') {
            clean_l_root_url = format!("/{}", clean_l_root_url);
        }
    }

    let target_dir = if l_root.is_empty() {
        ctx.cfg.source_dir.clone()
    } else if let Some(stripped) = l_root.strip_prefix('/') {
        ctx.cfg.source_dir.join(stripped)
    } else {
        ctx.cfg.source_dir.join(l_root.replace('.', "/"))
    };

    let mut list_html = String::from("<list class=\"list-pages\">\n<ul class=\"list-pages-ul\">\n");

    if target_dir.is_dir() {
        let mut matching_slugs: Vec<String> = Vec::new();
        let mut file_disp_names: HashMap<String, String> = HashMap::new();
        let mut file_weights: HashMap<String, String> = HashMap::new();
        let mut dir_implicit_weights: HashMap<String, String> = HashMap::new();

        for p_slug in &ctx.pages {
            let Some(pm) = ctx.page_meta.get(p_slug) else {
                continue;
            };
            let Ok(rel_p) = pm.file_path.strip_prefix(&target_dir) else {
                continue;
            };
            let rel_str = rel_p.to_string_lossy();
            let slashes = rel_str.matches('/').count();
            if l_depth > 0 && slashes >= l_depth {
                continue;
            }
            let fname = pm
                .file_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if l_pattern != "*" && l_pattern != "*.wiki" && !glob_match(&l_pattern, fname) {
                continue;
            }
            let stem = pm
                .file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let fname_no_ext = split_num_prefix(stem).map(|(_, r)| r).unwrap_or(stem);

            let rel_dir_raw = rel_p.parent().and_then(|d| d.to_str()).unwrap_or("");
            let mut clean_rel_dir = String::new();
            if !rel_dir_raw.is_empty() {
                for r_part_raw in rel_dir_raw.split('/') {
                    let (cw, r_part) =
                        split_num_prefix(r_part_raw).unwrap_or(("9999999", r_part_raw));
                    if clean_rel_dir.is_empty() {
                        clean_rel_dir.push_str(r_part);
                    } else {
                        clean_rel_dir.push('/');
                        clean_rel_dir.push_str(r_part);
                    }
                    let temp_slug = if !clean_l_root_slug.is_empty() {
                        format!("{}.{}", clean_l_root_slug, clean_rel_dir.replace('/', "."))
                    } else {
                        clean_rel_dir.replace('/', ".")
                    };
                    if cw != "9999999" {
                        dir_implicit_weights
                            .insert(temp_slug.trim_start_matches('.').to_string(), cw.to_string());
                    }
                }
            }

            let slug_key = p_slug.as_str();
            if match_exclude_expr(&l_exclude, slug_key, fname, stem, fname_no_ext) {
                continue;
            }

            let mut globals_ok = true;
            for (gk, req) in &l_globals {
                let actual = pm.vars.get(gk).map(|s| s.as_str()).unwrap_or("");
                if !match_global_req(actual, req) {
                    globals_ok = false;
                    break;
                }
            }
            if !globals_ok {
                continue;
            }

            let rel_slug = if clean_rel_dir.is_empty() {
                fname_no_ext.to_string()
            } else {
                format!("{}/{}", clean_rel_dir, fname_no_ext)
            };

            let mut disp_name = if let Some(gk) = l_page_label.strip_prefix("globals.") {
                pm.vars.get(gk).cloned().unwrap_or_default()
            } else if l_page_label == "name" {
                fname_no_ext.to_string()
            } else if l_page_label == "weight" {
                pm.weight.clone()
            } else {
                String::new()
            };
            if disp_name.is_empty() {
                disp_name = if !pm.title.is_empty() {
                    pm.title.clone()
                } else {
                    fname_no_ext.to_string()
                };
            }

            matching_slugs.push(rel_slug.clone());
            file_disp_names.insert(rel_slug.clone(), disp_name);
            file_weights.insert(rel_slug, pm.weight.clone());
        }

        if !matching_slugs.is_empty() {
            let mut raw_sorted: Vec<(String, String)> = Vec::with_capacity(matching_slugs.len());
            for rel_slug in &matching_slugs {
                let parts: Vec<&str> = rel_slug.split('/').collect();
                let mut sort_key = String::new();
                let mut cum_path = String::new();
                for (i, &part) in parts.iter().enumerate() {
                    if cum_path.is_empty() {
                        cum_path.push_str(part);
                    } else {
                        cum_path.push('/');
                        cum_path.push_str(part);
                    }
                    let (typ, w) = if i == parts.len() - 1 {
                        (
                            '1',
                            file_weights
                                .get(rel_slug)
                                .map(|s| s.as_str())
                                .unwrap_or("9999999")
                                .to_string(),
                        )
                    } else {
                        let dir_slug_key = if !clean_l_root_slug.is_empty() {
                            format!("{}.{}", clean_l_root_slug, cum_path.replace('/', "."))
                        } else {
                            cum_path.replace('/', ".")
                        };
                        let dsk = dir_slug_key.trim_start_matches('.');
                        let mut dw = ctx
                            .dir_weights
                            .get(dsk)
                            .or_else(|| ctx.page_meta.get(dsk).map(|m| &m.weight))
                            .map(|s| s.as_str())
                            .unwrap_or("9999999")
                            .to_string();
                        if dw == "9999999" {
                            if let Some(iw) = dir_implicit_weights.get(dsk) {
                                dw = iw.clone();
                            }
                        }
                        ('0', dw)
                    };
                    let padded = format!("{:0>7}", w);
                    let tail = &padded[padded.len().saturating_sub(7)..];
                    sort_key.push_str(&format!("/{}_{}_{}", typ, tail, part));
                }
                raw_sorted.push((sort_key, rel_slug.clone()));
            }
            raw_sorted.sort_by(|a, b| a.0.cmp(&b.0));

            let mut current_path: Vec<String> = Vec::new();
            for (_, rel_slug) in raw_sorted {
                let parts: Vec<&str> = rel_slug.split('/').collect();
                let dir_parts: Vec<&str> = parts[..parts.len() - 1].to_vec();
                let mut common_len = 0usize;
                while common_len < current_path.len()
                    && common_len < dir_parts.len()
                    && current_path[common_len] == dir_parts[common_len]
                {
                    common_len += 1;
                }
                for _ in common_len..current_path.len() {
                    list_html.push_str("</ul></li>\n");
                }
                let mut cum_dir = String::new();
                for (i, &dir) in dir_parts.iter().enumerate() {
                    if cum_dir.is_empty() {
                        cum_dir.push_str(dir);
                    } else {
                        cum_dir.push('/');
                        cum_dir.push_str(dir);
                    }
                    if i >= common_len {
                        let dir_slug_key = if !clean_l_root_slug.is_empty() {
                            format!("{}.{}", clean_l_root_slug, cum_dir.replace('/', "."))
                        } else {
                            cum_dir.replace('/', ".")
                        };
                        let dsk = dir_slug_key.trim_start_matches('.');
                        let mut dir_disp = ctx
                            .page_meta
                            .get(dsk)
                            .and_then(|m| {
                                if m.title.is_empty() {
                                    None
                                } else {
                                    Some(m.title.clone())
                                }
                            })
                            .unwrap_or_else(|| dir.to_string());
                        if !dir_disp.ends_with('/') {
                            dir_disp.push('/');
                        }
                        list_html.push_str(&format!(
                            "<li class=\"list-pages-dir\">{}<ul class=\"list-pages-ul-inner\">\n",
                            dir_disp
                        ));
                    }
                }
                let mut p_url = if !clean_l_root_url.is_empty() {
                    format!("{}/{}.html", clean_l_root_url, rel_slug)
                } else {
                    format!("{}.html", rel_slug)
                };
                if !p_url.starts_with('/') {
                    p_url = format!("/{}", p_url);
                }
                while p_url.contains("//") {
                    p_url = p_url.replace("//", "/");
                }
                let disp_name = file_disp_names
                    .get(&rel_slug)
                    .map(|s| s.as_str())
                    .unwrap_or(&rel_slug);
                list_html.push_str(&format!(
                    "<li class=\"list-pages-li\"><a href=\"{}\">{}</a></li>\n",
                    p_url, disp_name
                ));
                current_path = dir_parts.iter().map(|s| s.to_string()).collect();
            }
            for _ in 0..current_path.len() {
                list_html.push_str("</ul></li>\n");
            }
        }
    }

    list_html.push_str("</ul>\n</list>");
    list_html
}

pub fn counting_pages(ctx: &WikiContext, current_dir: &str, args: &str) -> usize {
    let mut l_root = current_dir.to_string();
    let mut l_pattern = "*".to_string();
    let mut l_exclude = String::new();
    let mut l_depth = 0usize;
    let mut l_globals: HashMap<String, String> = HashMap::new();

    for (key, val) in parse_kv_args(args) {
        match key.as_str() {
            "root" => l_root = val,
            "pattern" => l_pattern = val,
            "page_label" => {}
            "exclude" => l_exclude = val,
            "depth" => l_depth = val.parse::<usize>().unwrap_or(0),
            other => {
                let gk = other.strip_prefix("globals.").unwrap_or(other);
                l_globals.insert(gk.to_string(), val);
            }
        }
    }
    if l_root == "." || l_root == "/" {
        l_root.clear();
    }
    let target_dir = if l_root.is_empty() {
        ctx.cfg.source_dir.clone()
    } else if let Some(stripped) = l_root.strip_prefix('/') {
        ctx.cfg.source_dir.join(stripped)
    } else {
        ctx.cfg.source_dir.join(l_root.replace('.', "/"))
    };

    if !target_dir.is_dir() {
        return 0;
    }

    let mut count = 0usize;
    for p_slug in &ctx.pages {
        let Some(pm) = ctx.page_meta.get(p_slug) else {
            continue;
        };
        let Ok(rel_p) = pm.file_path.strip_prefix(&target_dir) else {
            continue;
        };
        let rel_str = rel_p.to_string_lossy();
        let slashes = rel_str.matches('/').count();
        if l_depth > 0 && slashes >= l_depth {
            continue;
        }
        let fname = pm
            .file_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if l_pattern != "*" && l_pattern != "*.wiki" && !glob_match(&l_pattern, fname) {
            continue;
        }
        let stem = pm
            .file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let fname_no_ext = split_num_prefix(stem).map(|(_, r)| r).unwrap_or(stem);
        if match_exclude_expr(&l_exclude, p_slug, fname, stem, fname_no_ext) {
            continue;
        }
        let mut globals_ok = true;
        for (gk, req) in &l_globals {
            let actual = pm.vars.get(gk).map(|s| s.as_str()).unwrap_or("");
            if !match_global_req(actual, req) {
                globals_ok = false;
                break;
            }
        }
        if globals_ok {
            count += 1;
        }
    }
    count
}

fn collect_block_target_files(
    ctx: &WikiContext,
    current_file: &Path,
    l_root: &str,
    l_depth: usize,
    l_page: &str,
    l_exclude: &str,
) -> Vec<(PathBuf, String, String)> {
    let mut target_files: Vec<PathBuf> = Vec::new();
    if !l_page.is_empty() {
        let base = ctx.cfg.source_dir.join(l_page.replace('.', "/"));
        for cand in [
            base.with_extension("md"),
            base.with_extension("wiki"),
            base,
        ] {
            if cand.is_file() {
                target_files.push(cand);
                break;
            }
        }
    } else if !l_root.is_empty() {
        let t_dir = if l_root == "/" {
            ctx.cfg.source_dir.clone()
        } else if let Some(stripped) = l_root.strip_prefix('/') {
            ctx.cfg.source_dir.join(stripped)
        } else {
            ctx.cfg.source_dir.join(l_root.replace('.', "/"))
        };
        if t_dir.is_dir() {
            for p in &ctx.pages {
                if let Some(pm) = ctx.page_meta.get(p) {
                    if let Ok(rel) = pm.file_path.strip_prefix(&t_dir) {
                        let d_count = rel.to_string_lossy().matches('/').count();
                        if d_count < l_depth {
                            target_files.push(pm.file_path.clone());
                        }
                    }
                }
            }
        }
    } else {
        target_files.push(current_file.to_path_buf());
    }

    target_files.sort();

    let mut out = Vec::with_capacity(target_files.len());
    for tf in target_files {
        if !tf.is_file() {
            continue;
        }
        let rel = tf.strip_prefix(&ctx.cfg.source_dir).unwrap_or(&tf);
        let fname = tf.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let stem = tf.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let fname_no_ext = split_num_prefix(stem).map(|(_, r)| r).unwrap_or(stem);

        let rel_dir_raw = rel.parent().and_then(|p| p.to_str()).unwrap_or("");
        let mut clean_rel_dir = String::new();
        if !rel_dir_raw.is_empty() {
            for part in rel_dir_raw.split('/') {
                let p = split_num_prefix(part).map(|(_, r)| r).unwrap_or(part);
                if clean_rel_dir.is_empty() {
                    clean_rel_dir.push_str(p);
                } else {
                    clean_rel_dir.push('/');
                    clean_rel_dir.push_str(p);
                }
            }
        }

        let slug_key = if clean_rel_dir.is_empty() {
            fname_no_ext.to_string()
        } else {
            format!("{}.{}", clean_rel_dir.replace('/', "."), fname_no_ext)
        };

        if match_exclude_expr(l_exclude, &slug_key, fname, stem, fname_no_ext) {
            continue;
        }

        let t_url = if clean_rel_dir.is_empty() {
            format!("/{}.html", fname_no_ext)
        } else {
            format!("/{}/{}.html", clean_rel_dir, fname_no_ext)
        };
        let disp_path = if clean_rel_dir.is_empty() {
            fname_no_ext.to_string()
        } else {
            format!("{}/{}", clean_rel_dir, fname_no_ext)
        };
        out.push((tf, t_url, disp_path));
    }
    out
}

pub fn listing_blocks(ctx: &WikiContext, current_file: &Path, b_type: &str, args: &str) -> String {
    let singular = b_type.strip_suffix('s').unwrap_or(b_type);
    let mut l_root = String::new();
    let mut l_depth = 999usize;
    let mut l_page = String::new();
    let mut l_exclude = String::new();
    let mut l_context = false;

    if !args.is_empty() && !args.contains('=') {
        l_page = args.trim().to_string();
    } else {
        for (k, v) in parse_kv_args(args) {
            match k.as_str() {
                "root" => l_root = v,
                "depth" => l_depth = v.parse::<usize>().unwrap_or(999),
                "page" => l_page = v,
                "exclude" => l_exclude = v,
                "context" => l_context = v.eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
    }

    let flat_list = l_root.is_empty();
    let mut list_html = format!("<list class=\"list-block-{}\">\n", singular);
    if flat_list {
        list_html.push_str(&format!("<ul class=\"list-block-{}-ul\">\n", singular));
    } else {
        list_html.push_str(&format!("<ol class=\"list-block-{}-files\">\n", singular));
    }

    let files = collect_block_target_files(ctx, current_file, &l_root, l_depth, &l_page, &l_exclude);
    for (tf, t_url, disp_path) in files {
        let Ok(text) = fs::read_to_string(&tf) else {
            continue;
        };
        let items = scan_blocks_in_file(&text, b_type, singular, &t_url, l_context);
        if items.is_empty() {
            continue;
        }
        if flat_list {
            for li in items {
                list_html.push_str("            ");
                list_html.push_str(&li);
                list_html.push('\n');
            }
        } else {
            list_html.push_str(&format!(
                "    <li class=\"list-block-{}-file\">{}\n        <ul class=\"list-block-{}-ul\">\n",
                singular, disp_path, singular
            ));
            for li in items {
                list_html.push_str("            ");
                list_html.push_str(&li);
                list_html.push('\n');
            }
            list_html.push_str("        </ul>\n    </li>\n");
        }
    }

    if flat_list {
        list_html.push_str("</ul>\n</list>");
    } else {
        list_html.push_str("</ol>\n</list>");
    }
    list_html
}

fn scan_blocks_in_file(
    text: &str,
    b_type: &str,
    singular: &str,
    t_url: &str,
    l_context: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = String::new();
    let mut nest = 0usize;
    let mut item_count_table = 0usize;
    let mut item_count_diagram = 0usize;

    let mut block_id = String::new();
    let mut first_key = String::new();
    let mut first_val = String::new();
    let mut b_context = String::new();
    let mut b_entity = String::new();
    let mut t_buf: Vec<String> = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if in_block.is_empty() {
            let trimmed = line.trim();
            if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
                let inner = trimmed[2..trimmed.len() - 2].trim();
                if let Some(m) = inner.strip_prefix("block.") {
                    if !m.is_empty()
                        && m.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                    {
                        if m == "table" {
                            item_count_table += 1;
                        }
                        if m == "diagram" {
                            item_count_diagram += 1;
                        }
                        let is_match = m == b_type || m == singular || format!("{}s", m) == b_type;
                        if is_match {
                            in_block = m.to_string();
                            nest = 0;
                            block_id.clear();
                            first_key.clear();
                            first_val.clear();
                            b_context.clear();
                            b_entity.clear();
                            t_buf.clear();
                        }
                    }
                }
            }
        } else {
            let mut c_line = line.trim_start();
            if let Some(after) = c_line.strip_prefix("::") {
                c_line = after.trim_start();
            }
            let r_trim = c_line.trim_end();
            if r_trim.starts_with("{{") && r_trim.ends_with("}}") {
                let inner = r_trim[2..r_trim.len() - 2].trim();
                if inner == "block.end" {
                    if nest > 0 {
                        nest -= 1;
                        continue;
                    }
                    if in_block == "table" {
                        let mut sep_count = 0usize;
                        for tl in &t_buf {
                            let ns: String = tl.chars().filter(|&c| c != ' ').collect();
                            if ns.len() >= 4 && ns.chars().all(|c| matches!(c, '-' | '=' | '_')) {
                                sep_count += 1;
                            }
                        }
                        let mut caption = String::new();
                        if sep_count >= 2 {
                            let mut found = 0usize;
                            for tl in &t_buf {
                                let ns: String = tl.chars().filter(|&c| c != ' ').collect();
                                if ns.len() >= 4
                                    && ns.chars().all(|c| matches!(c, '-' | '=' | '_'))
                                {
                                    found += 1;
                                    continue;
                                }
                                if found == sep_count {
                                    let cl = tl.trim();
                                    if !cl.is_empty() {
                                        caption = cl.to_string();
                                        break;
                                    }
                                }
                            }
                        }
                        let cap_txt = if caption.is_empty() {
                            format!("Table {}", item_count_table)
                        } else {
                            caption
                        };
                        out.push(format!(
                            "<li class=\"list-block-{}-li\"><a href=\"{}#table-{}\">{}</a></li>",
                            singular, t_url, item_count_table, cap_txt
                        ));
                    } else if in_block == "diagram" {
                        out.push(format!(
                            "<li class=\"list-block-{}-li\"><a href=\"{}#diagram-{}\">Diagram {}</a></li>",
                            singular, t_url, item_count_diagram, item_count_diagram
                        ));
                    } else {
                        let pre_ctx = if !b_context.is_empty() && l_context {
                            format!("{}: ", b_context)
                        } else {
                            String::new()
                        };
                        let mut link_txt = if singular == "def" {
                            first_val.clone()
                        } else if !b_entity.is_empty() {
                            b_entity.clone()
                        } else {
                            first_val.clone()
                        };
                        let raw_id = if !block_id.is_empty() {
                            &block_id
                        } else if !b_entity.is_empty() {
                            &b_entity
                        } else {
                            &first_val
                        };
                        let mut ref_key = slugify(raw_id);
                        if !b_context.is_empty() && !raw_id.contains('.') {
                            ref_key = slugify(&format!("{}.{}", b_context, raw_id));
                        }
                        let anc_slug = ref_key.replace(['/', '.'], "-");
                        let anc_id = format!("block-{}-{}", in_block, anc_slug);
                        if link_txt.is_empty() {
                            link_txt = anc_id.clone();
                        }
                        out.push(format!(
                            "<li class=\"list-block-{}-li\">{}<a href=\"{}#{}\">{}</a></li>",
                            singular, pre_ctx, t_url, anc_id, link_txt
                        ));
                    }
                    in_block.clear();
                    continue;
                } else if inner.starts_with("block.") {
                    nest += 1;
                    continue;
                }
            }

            if in_block == "table" {
                t_buf.push(line.to_string());
            } else if nest == 0 && line.trim_start().starts_with("::") {
                let after = line.trim_start()[2..].trim_start();
                if let Some(idx) = after.find(':') {
                    let k = &after[..idx];
                    if !k.is_empty()
                        && k.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                    {
                        let v = strip_quotes(after[idx + 1..].trim()).to_string();
                        match k {
                            "__id__" => block_id = v,
                            "__context__" | "context" => b_context = v,
                            "__entity__" | "__entry__" => b_entity = v,
                            _ => {
                                if first_key.is_empty() {
                                    first_key = k.to_string();
                                    first_val = v;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

pub fn counting_blocks(ctx: &WikiContext, current_file: &Path, b_type: &str, args: &str) -> usize {
    let singular = b_type.strip_suffix('s').unwrap_or(b_type);
    let mut l_root = String::new();
    let mut l_depth = 999usize;
    let mut l_page = String::new();
    let mut l_exclude = String::new();

    if !args.is_empty() && !args.contains('=') {
        l_page = args.trim().to_string();
    } else {
        for (k, v) in parse_kv_args(args) {
            match k.as_str() {
                "root" => l_root = v,
                "depth" => l_depth = v.parse::<usize>().unwrap_or(999),
                "page" => l_page = v,
                "exclude" => l_exclude = v,
                _ => {}
            }
        }
    }

    let files = collect_block_target_files(ctx, current_file, &l_root, l_depth, &l_page, &l_exclude);
    let mut total = 0usize;

    for (tf, _, _) in files {
        let Ok(text) = fs::read_to_string(&tf) else {
            continue;
        };
        let mut in_b = false;
        let mut nest = 0usize;
        for raw_line in text.lines() {
            if !raw_line.contains("block.") {
                continue;
            }
            let line = raw_line.trim_end_matches('\r');
            if !in_b {
                let trimmed = line.trim();
                if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
                    let inner = trimmed[2..trimmed.len() - 2].trim();
                    if let Some(m) = inner.strip_prefix("block.") {
                        if m == b_type || m == singular || format!("{}s", m) == b_type {
                            in_b = true;
                            nest = 0;
                        }
                    }
                }
            } else {
                let mut c_line = line.trim_start();
                if let Some(after) = c_line.strip_prefix("::") {
                    c_line = after.trim_start();
                }
                let r_trim = c_line.trim_end();
                if r_trim.starts_with("{{") && r_trim.ends_with("}}") {
                    let inner = r_trim[2..r_trim.len() - 2].trim();
                    if inner == "block.end" {
                        if nest > 0 {
                            nest -= 1;
                        } else {
                            total += 1;
                            in_b = false;
                        }
                    } else if inner.starts_with("block.") {
                        nest += 1;
                    }
                }
            }
        }
    }
    total
}
