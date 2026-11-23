use crate::model::{
    char_len, extract_svg_viewbox_width, get_or_render_latex, html_escape, slugify, strip_quotes,
    RenderState, WikiContext,
};
use std::collections::HashMap;

struct DiagramNode {
    name: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    border: String,
    bg: String,
    n_class: String,
    has_tex: bool,
    raw_text: String,
    html_text: String,
}

struct DiagramArrow {
    a_type: String,
    name: String,
    src: String,
    tgt: String,
    has_coords: bool,
    ax: f64,
    ay: f64,
    format: String,
    style: String,
    angle_raw: String,
    color: String,
    size: f64,
    size_explicit: bool,
    a_class: String,
    label_pos: String,
    has_tex: bool,
    label_raw: String,
    label_html: String,
    label_w: f64,
    label_h: f64,
    loop_dir: String,
}

fn split_diagram_props(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q: Option<char> = None;
    for ch in s.chars() {
        if let Some(q) = in_q {
            if ch == q {
                in_q = None;
            }
            cur.push(ch);
        } else if ch == '"' || ch == '\'' {
            in_q = Some(ch);
            cur.push(ch);
        } else if ch == ',' {
            let trimmed = cur.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    let trimmed = cur.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    out
}

fn diagram_render_tex(ctx: &WikiContext, s: &str) -> (String, i64, bool) {
    if !s.contains('$') {
        return (s.to_string(), 0, false);
    }
    let mut out = String::new();
    let mut work = s;
    let mut has_tex = false;
    let mut total_w = 0i64;

    while let Some(i1) = work.find('$') {
        let prefix = &work[..i1];
        let rest = &work[i1 + 1..];
        let Some(i2) = rest.find('$') else {
            break;
        };
        let math_code = &rest[..i2];
        if math_code.is_empty() {
            out.push_str(prefix);
            out.push_str("$$");
            work = &rest[i2 + 1..];
            continue;
        }
        has_tex = true;
        let mut pt_w = 20i64;
        let span = if let Some(cached) = get_or_render_latex(ctx, math_code, true) {
            if let Some(vw) = extract_svg_viewbox_width(&cached) {
                pt_w = vw;
            }
            cached
        } else {
            format!("${}$", math_code)
        };
        total_w += pt_w;
        out.push_str(prefix);
        out.push_str(&span);
        work = &rest[i2 + 1..];
    }
    if !has_tex {
        return (s.to_string(), 0, false);
    }
    out.push_str(work);
    (out, total_w, true)
}

fn strip_tex_segments(s: &str) -> String {
    let mut out = String::new();
    let mut work = s;
    while let Some(i1) = work.find('$') {
        out.push_str(&work[..i1]);
        let rest = &work[i1 + 1..];
        if let Some(i2) = rest.find('$') {
            work = &rest[i2 + 1..];
        } else {
            out.push('$');
            out.push_str(rest);
            return out;
        }
    }
    out.push_str(work);
    out
}

pub fn render_diagram(
    state: &mut RenderState,
    lines: &[String],
    nested: bool,
    inherited_ctx: &str,
) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let cls_prefix = if nested { "block-diagram" } else { "diagram" };
    let mut expanded_lines: Vec<String> = Vec::new();

    for line in lines {
        if line.contains(" :: ") {
            for sub in line.split(" :: ") {
                let s = sub.trim();
                if !s.is_empty() {
                    if s.starts_with("::") {
                        expanded_lines.push(s.to_string());
                    } else {
                        expanded_lines.push(format!(":: {}", s));
                    }
                }
            }
        } else {
            expanded_lines.push(line.clone());
        }
    }

    let mut diagram_caption = String::new();
    let mut diagram_raw_id = String::new();
    let mut diagram_ctx = inherited_ctx.to_string();
    let mut nodes: Vec<DiagramNode> = Vec::new();
    let mut arrows: Vec<DiagramArrow> = Vec::new();

    for line in &expanded_lines {
        let mut d_line = line.trim_start();
        while let Some(rest) = d_line.strip_prefix("::") {
            let trimmed = rest.trim_start();
            if trimmed.starts_with("::") {
                d_line = trimmed;
            } else {
                break;
            }
        }
        let Some(after_colons) = d_line.strip_prefix("::") else {
            continue;
        };
        let after_colons = after_colons.trim_start();

        if let Some((head, tail)) = after_colons.split_once(':') {
            let key = head.trim();
            if key == "__id__" || key == "id" {
                diagram_raw_id = strip_quotes(tail).to_string();
                continue;
            } else if key == "__context__" || key == "context" {
                diagram_ctx = strip_quotes(tail).to_string();
                continue;
            } else if key == "caption" {
                diagram_caption = strip_quotes(tail).to_string();
                continue;
            }
        }

        let (cmd, raw_content) = if let Some(rest) = after_colons.strip_prefix("2-arrow") {
            let rest = rest.trim_start();
            ("2-arrow", rest.strip_prefix(':').unwrap_or(rest).trim_start())
        } else if let Some(rest) = after_colons.strip_prefix("arrow") {
            let rest = rest.trim_start();
            ("arrow", rest.strip_prefix(':').unwrap_or(rest).trim_start())
        } else if let Some(rest) = after_colons.strip_prefix("node") {
            let rest = rest.trim_start();
            ("node", rest.strip_prefix(':').unwrap_or(rest).trim_start())
        } else if let Some(rest) = after_colons.strip_prefix("term") {
            let rest = rest.trim_start();
            ("term", rest.strip_prefix(':').unwrap_or(rest).trim_start())
        } else {
            continue;
        };

        if cmd == "node" || cmd == "term" {
            let tokens = split_diagram_props(raw_content);
            if tokens.is_empty() {
                continue;
            }
            let name = strip_quotes(&tokens[0]).to_string();
            let mut x = 0.0f64;
            let mut y = 0.0f64;
            let mut border = "none".to_string();
            let mut bg = "default".to_string();
            let mut n_class = String::new();
            let mut n_text = String::new();
            let mut x_set = false;
            let mut y_set = false;

            for tok in tokens.iter().skip(1) {
                if let Some((k_raw, v_raw)) = tok.split_once('=') {
                    let k = k_raw.trim();
                    let v = strip_quotes(v_raw);
                    match k {
                        "x" => {
                            x = v.parse::<f64>().unwrap_or(0.0);
                            x_set = true;
                        }
                        "y" => {
                            y = v.parse::<f64>().unwrap_or(0.0);
                            y_set = true;
                        }
                        "border" => border = v.to_string(),
                        "bg" => bg = v.to_string(),
                        "class" => n_class = v.to_string(),
                        "text" => n_text = v.to_string(),
                        _ => {}
                    }
                } else {
                    let clean = strip_quotes(tok);
                    if let Ok(num) = clean.parse::<f64>() {
                        if !x_set {
                            x = num;
                            x_set = true;
                        } else if !y_set {
                            y = num;
                            y_set = true;
                        }
                    } else if matches!(clean, "none" | "rounded" | "squared") {
                        border = clean.to_string();
                    }
                }
            }

            let display_text = if n_text.is_empty() {
                name.clone()
            } else {
                n_text
            };
            let (html_text, tex_w, has_tex) = diagram_render_tex(state.ctx, &display_text);
            let text_lines: Vec<&str> = display_text.split("\\n").collect();
            let mut max_len = 0usize;
            for l_str in &text_lines {
                let no_tex = strip_tex_segments(l_str);
                let clen = char_len(&no_tex);
                if clen > max_len {
                    max_len = clen;
                }
            }
            let line_count = text_lines.len().max(1) as i64;
            let (w, h) = if border == "none" || border.is_empty() {
                let mut w_i = ((max_len as i64 * 85 + 5) / 10) + tex_w + 10;
                if w_i < 18 {
                    w_i = 18;
                }
                let mut h_i = line_count * 18 + 6;
                if has_tex && h_i < 26 {
                    h_i = 26;
                }
                (w_i as f64, h_i as f64)
            } else {
                let mut w_i = ((max_len as i64 * 85 + 5) / 10) + tex_w + 28;
                if w_i < 72 {
                    w_i = 72;
                }
                let mut h_i = line_count * 18 + 18;
                if has_tex && h_i < 42 {
                    h_i = 42;
                }
                (w_i as f64, h_i as f64)
            };

            nodes.push(DiagramNode {
                name,
                x,
                y,
                w,
                h,
                border,
                bg,
                n_class,
                has_tex,
                raw_text: display_text,
                html_text,
            });
        } else {
            let tokens = split_diagram_props(raw_content);
            let mut positional: Vec<String> = Vec::new();
            let mut prop_tokens: Vec<String> = Vec::new();

            for tok in &tokens {
                if tok.contains('=') {
                    prop_tokens.push(tok.clone());
                } else {
                    let clean = strip_quotes(tok);
                    if matches!(
                        clean,
                        "dotted"
                            | "dashed"
                            | "wavy"
                            | "tilded"
                            | "curly"
                            | "straight"
                            | "diagonal"
                            | "diag"
                            | "normal"
                    ) {
                        prop_tokens.push(clean.to_string());
                    } else {
                        positional.push(clean.to_string());
                    }
                }
            }

            let mut src = String::new();
            let mut tgt = String::new();
            let a_name = if positional.len() >= 3 {
                src = positional[1].clone();
                tgt = positional[2].clone();
                positional[0].clone()
            } else if positional.len() == 2 {
                src = positional[0].clone();
                tgt = positional[1].clone();
                format!("{}_{}", cmd, arrows.len())
            } else if positional.len() == 1 {
                positional[0].clone()
            } else {
                format!("{}_{}", cmd, arrows.len())
            };

            let mut a_format = "normal".to_string();
            let mut a_angle = String::new();
            let mut a_color = String::new();
            let mut a_label = String::new();
            let mut a_label_pos = "above".to_string();
            let mut a_size = String::new();
            let mut a_class = String::new();
            let mut l_style = "continuous".to_string();
            let mut ax = 0.0f64;
            let mut ay = 0.0f64;
            let mut has_coords = false;
            let mut a_loop_dir = "top".to_string();

            for tok in &prop_tokens {
                if let Some((k_raw, v_raw)) = tok.split_once('=') {
                    let k = k_raw.trim();
                    if k == "label" {
                        let mut v = v_raw.trim();
                        if let Some(s) = v.strip_prefix('^') {
                            a_label_pos = "above".to_string();
                            v = s;
                        } else if let Some(s) = v.strip_prefix('_') {
                            a_label_pos = "below".to_string();
                            v = s;
                        }
                        let mut stripped = strip_quotes(v);
                        if let Some(s) = stripped.strip_prefix('^') {
                            a_label_pos = "above".to_string();
                            stripped = s;
                        } else if let Some(s) = stripped.strip_prefix('_') {
                            a_label_pos = "below".to_string();
                            stripped = s;
                        }
                        a_label = stripped.to_string();
                    } else {
                        let v = strip_quotes(v_raw);
                        match k {
                            "style" => l_style = v.to_string(),
                            "format" => a_format = v.to_string(),
                            "angle" => a_angle = v.to_string(),
                            "color" => a_color = v.to_string(),
                            "class" => a_class = v.to_string(),
                            "source" | "s" => src = v.to_string(),
                            "target" | "t" => tgt = v.to_string(),
                            "size" => a_size = v.to_string(),
                            "pos" | "dir" => a_loop_dir = v.to_string(),
                            "x" => {
                                ax = v.parse::<f64>().unwrap_or(0.0);
                                has_coords = true;
                            }
                            "y" => {
                                ay = v.parse::<f64>().unwrap_or(0.0);
                                has_coords = true;
                            }
                            _ => {}
                        }
                    }
                } else {
                    match tok.as_str() {
                        "dotted" | "dashed" | "wavy" => l_style = tok.clone(),
                        "tilded" => l_style = "wavy".to_string(),
                        "curly" | "straight" | "diagonal" | "diag" | "normal" => {
                            a_format = tok.clone()
                        }
                        _ => {}
                    }
                }
            }

            if a_format == "diag" || a_format == "diagonal" {
                a_format = "straight".to_string();
            }
            let size_explicit = !a_size.is_empty();
            let size_val = if size_explicit {
                a_size.parse::<f64>().unwrap_or(0.9)
            } else if cmd == "2-arrow" {
                0.6
            } else {
                0.9
            };

            let mut label_html = String::new();
            let mut has_tex = false;
            let mut label_w = 0.0f64;
            let mut label_h = 0.0f64;

            if !a_label.is_empty() {
                let (lh, tex_w, htex) = diagram_render_tex(state.ctx, &a_label);
                label_html = lh;
                has_tex = htex;
                let lbl_lines: Vec<&str> = a_label.split("\\n").collect();
                let mut max_lbl_len = 0usize;
                for l_str in &lbl_lines {
                    let no_tex = strip_tex_segments(l_str);
                    let clen = char_len(&no_tex);
                    if clen > max_lbl_len {
                        max_lbl_len = clen;
                    }
                }
                let lbl_line_count = lbl_lines.len().max(1) as i64;
                let lw = ((max_lbl_len as i64 * 85 + 5) / 10) + tex_w + 16;
                let mut lhv = lbl_line_count * 18;
                if has_tex && lhv < 28 {
                    lhv = 28;
                }
                label_w = lw as f64;
                label_h = lhv as f64;
            }

            arrows.push(DiagramArrow {
                a_type: cmd.to_string(),
                name: a_name,
                src,
                tgt,
                has_coords,
                ax,
                ay,
                format: a_format,
                style: l_style,
                angle_raw: a_angle,
                color: a_color,
                size: size_val,
                size_explicit,
                a_class,
                label_pos: a_label_pos,
                has_tex,
                label_raw: a_label,
                label_html,
                label_w,
                label_h,
                loop_dir: a_loop_dir,
            });
        }
    }

    if nodes.is_empty() {
        return String::new();
    }

    state.page_diagram_count += 1;
    let diagram_id = if !diagram_raw_id.is_empty() {
        let clean_id = slugify(&diagram_raw_id);
        let ref_key = if !diagram_ctx.is_empty() && !diagram_raw_id.contains('.') {
            slugify(&format!("{}.{}", diagram_ctx, diagram_raw_id))
        } else {
            clean_id
        };
        let anchor_slug = ref_key.replace(['/', '.'], "-");
        format!("block-diagram-{}", anchor_slug)
    } else {
        format!("{}-{}", cls_prefix, state.page_diagram_count)
    };

    let safe_caption = html_escape(&diagram_caption);
    let aria_label = if safe_caption.is_empty() {
        format!("Diagram {}", state.page_diagram_count)
    } else {
        safe_caption.clone()
    };

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut max_node_w = 0.0f64;
    let mut max_node_h = 0.0f64;
    let mut ux_vec: Vec<f64> = Vec::new();
    let mut uy_vec: Vec<f64> = Vec::new();

    for n in &nodes {
        min_x = min_x.min(n.x);
        max_x = max_x.max(n.x);
        min_y = min_y.min(n.y);
        max_y = max_y.max(n.y);
        max_node_w = max_node_w.max(n.w);
        max_node_h = max_node_h.max(n.h);
        ux_vec.push(n.x);
        uy_vec.push(n.y);
    }

    let mut pair_count: HashMap<(String, String), usize> = HashMap::new();
    for a in &arrows {
        if !a.src.is_empty() && !a.tgt.is_empty() && a.src != a.tgt && !a.has_coords {
            *pair_count
                .entry((a.src.clone(), a.tgt.clone()))
                .or_insert(0) += 1;
        }
        if a.has_coords {
            min_x = min_x.min(a.ax);
            max_x = max_x.max(a.ax);
            min_y = min_y.min(a.ay);
            max_y = max_y.max(a.ay);
            ux_vec.push(a.ax);
            uy_vec.push(a.ay);
        }
    }

    ux_vec.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    uy_vec.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut min_dx = 1.0f64;
    if ux_vec.len() > 1 {
        let mut best = 999999.0f64;
        for w in ux_vec.windows(2) {
            let d = w[1] - w[0];
            if d > 0.00001 && d < best {
                best = d;
            }
        }
        if best < 999999.0 {
            min_dx = best;
        }
    }
    let mut min_dy = 1.0f64;
    if uy_vec.len() > 1 {
        let mut best = 999999.0f64;
        for w in uy_vec.windows(2) {
            let d = w[1] - w[0];
            if d > 0.00001 && d < best {
                best = d;
            }
        }
        if best < 999999.0 {
            min_dy = best;
        }
    }

    let target_dist_x = (max_node_w + 48.0).max(130.0);
    let target_dist_y = (max_node_h + 38.0).max(76.0);
    let scale_x = if min_dx < 1.0 {
        target_dist_x / min_dx
    } else {
        target_dist_x
    };
    let scale_y = if min_dy < 1.0 {
        target_dist_y / min_dy
    } else {
        target_dist_y
    };

    let padding_x = max_node_w / 2.0 + 16.0;
    let padding_y = max_node_h / 2.0 + 16.0;

    let mut bound_min_x = f64::INFINITY;
    let mut bound_max_x = f64::NEG_INFINITY;
    let mut bound_min_y = f64::INFINITY;
    let mut bound_max_y = f64::NEG_INFINITY;

    let mut terms_x: HashMap<String, f64> = HashMap::new();
    let mut terms_y: HashMap<String, f64> = HashMap::new();
    let mut terms_w: HashMap<String, f64> = HashMap::new();
    let mut terms_h: HashMap<String, f64> = HashMap::new();
    let mut terms_border: HashMap<String, String> = HashMap::new();

    for n in &nodes {
        let px = (n.x - min_x) * scale_x + padding_x;
        let py = (max_y - n.y) * scale_y + padding_y;
        terms_x.insert(n.name.clone(), px);
        terms_y.insert(n.name.clone(), py);
        terms_w.insert(n.name.clone(), n.w);
        terms_h.insert(n.name.clone(), n.h);
        terms_border.insert(n.name.clone(), n.border.clone());

        bound_min_x = bound_min_x.min(px - n.w / 2.0);
        bound_max_x = bound_max_x.max(px + n.w / 2.0);
        bound_min_y = bound_min_y.min(py - n.h / 2.0);
        bound_max_y = bound_max_y.max(py + n.h / 2.0);
    }

    let wavy_id = format!("{}-wavy", diagram_id);
    let mut defs_html = format!(
        "<defs>\n    <filter id=\"{}\" filterUnits=\"userSpaceOnUse\" x=\"-20%\" y=\"-20%\" width=\"140%\" height=\"140%\">\n        <feTurbulence type=\"fractalNoise\" baseFrequency=\"0.05\" numOctaves=\"1\" result=\"noise\"/>\n        <feDisplacementMap in=\"SourceGraphic\" in2=\"noise\" scale=\"5\" xChannelSelector=\"R\" yChannelSelector=\"G\"/>\n    </filter>\n</defs>",
        wavy_id
    );

    let mut layer_arrows = String::new();
    let mut layer_labels = String::new();
    let mut pair_seen: HashMap<(String, String), usize> = HashMap::new();

    for pass in 1..=2 {
        for (i, a) in arrows.iter().enumerate() {
            if (pass == 1 && a.a_type == "2-arrow") || (pass == 2 && a.a_type == "arrow") {
                continue;
            }
            let (mut sx, mut sy, mut hw_s, mut hh_s) = (0.0, 0.0, 0.0, 0.0);
            let mut has_src = false;
            if let Some(&px) = terms_x.get(&a.src) {
                sx = px;
                sy = terms_y[&a.src];
                hw_s = terms_w[&a.src] / 2.0;
                hh_s = terms_h[&a.src] / 2.0;
                has_src = true;
            } else if a.has_coords {
                sx = (a.ax - min_x) * scale_x + padding_x;
                sy = (max_y - a.ay) * scale_y + padding_y;
                has_src = true;
            }

            let (mut tx, mut ty, mut hw_t, mut hh_t) = (0.0, 0.0, 0.0, 0.0);
            let mut has_tgt = false;
            if let Some(&px) = terms_x.get(&a.tgt) {
                tx = px;
                ty = terms_y[&a.tgt];
                hw_t = terms_w[&a.tgt] / 2.0;
                hh_t = terms_h[&a.tgt] / 2.0;
                has_tgt = true;
            } else if a.has_coords {
                tx = (a.ax - min_x) * scale_x + padding_x;
                ty = (max_y - a.ay) * scale_y + padding_y;
                has_tgt = true;
            }

            if !has_src || !has_tgt {
                if a.has_coords {
                    let apx = (a.ax - min_x) * scale_x + padding_x;
                    let apy = (max_y - a.ay) * scale_y + padding_y;
                    terms_x.insert(a.name.clone(), apx);
                    terms_y.insert(a.name.clone(), apy);
                    terms_w.insert(a.name.clone(), 0.0);
                    terms_h.insert(a.name.clone(), 0.0);
                }
                continue;
            }

            let mut a_format = a.format.clone();
            let mut a_label_pos = a.label_pos.clone();
            let src_no_border = terms_border
                .get(&a.src)
                .map(|b| b == "none" || b.is_empty())
                .unwrap_or(false);
            let tgt_no_border = terms_border
                .get(&a.tgt)
                .map(|b| b == "none" || b.is_empty())
                .unwrap_or(false);

            let has_angle = !a.angle_raw.is_empty();
            let mut a_angle = if has_angle {
                a.angle_raw.parse::<f64>().unwrap_or(45.0)
            } else {
                45.0
            };
            let mut auto_curved = false;

            if !a.has_coords && a.src != a.tgt && a_format == "normal" {
                let fwd = *pair_count
                    .get(&(a.src.clone(), a.tgt.clone()))
                    .unwrap_or(&0);
                let rev = *pair_count
                    .get(&(a.tgt.clone(), a.src.clone()))
                    .unwrap_or(&0);
                if rev > 0 || fwd > 1 {
                    a_format = "curly".to_string();
                    if !has_angle {
                        a_angle = 20.0;
                        auto_curved = true;
                    }
                    if fwd > 1 {
                        let idx_p = {
                            let e = pair_seen.entry((a.src.clone(), a.tgt.clone())).or_insert(0);
                            *e += 1;
                            *e
                        };
                        if idx_p % 2 == 0 {
                            a_angle = -a_angle * ((idx_p / 2) as f64);
                        } else if idx_p > 1 {
                            a_angle *= ((idx_p + 1) / 2) as f64;
                        }
                    }
                }
            }

            let path_id = format!("{}-path-{}", diagram_id, i);
            let marker_id = format!("{}-marker-{}", diagram_id, i);

            let cp = cls_prefix;
            let (path_class_attr, path_color_attr, marker_fill_attr, label_color_attr) =
                if !a.color.is_empty() && a.color != "currentColor" {
                    let cls = if a.a_class.is_empty() {
                        String::new()
                    } else {
                        format!(" class=\"{}\"", a.a_class)
                    };
                    (
                        cls.clone(),
                        format!(" stroke=\"{}\"", a.color),
                        format!("{} fill=\"{}\"", cls, a.color),
                        format!(" fill=\"{}\"", a.color),
                    )
                } else {
                    let extra = if a.a_class.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", a.a_class)
                    };
                    (
                        format!(" class=\"{}-path{}\"", cp, extra),
                        String::new(),
                        format!(" class=\"{}-marker{}\"", cp, extra),
                        String::new(),
                    )
                };

            let label_class_attr = if a.a_class.is_empty() {
                format!(" class=\"{}-text {}-label\"", cp, cp)
            } else {
                format!(" class=\"{}-text {}-label {}\"", cp, cp, a.a_class)
            };

            if a.a_type == "2-arrow" {
                let m_stroke = if !a.color.is_empty() && a.color != "currentColor" {
                    let cls = if a.a_class.is_empty() {
                        String::new()
                    } else {
                        format!(" class=\"{}\"", a.a_class)
                    };
                    format!("{} stroke=\"{}\"", cls, a.color)
                } else {
                    let extra = if a.a_class.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", a.a_class)
                    };
                    format!(" class=\"{}-path{}\"", cp, extra)
                };
                defs_html.push_str(&format!(
                    "<marker id=\"{}\" viewBox=\"0 0 10 10\" refX=\"6\" refY=\"5\" markerWidth=\"8\" markerHeight=\"8\" orient=\"auto-start-reverse\"><path d=\"M 0 1 L 7 5 L 0 9\" fill=\"none\"{} stroke-width=\"1.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/></marker>\n",
                    marker_id, m_stroke
                ));
            } else {
                defs_html.push_str(&format!(
                    "<marker id=\"{}\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\"><path d=\"M 0 0 L 10 5 L 0 10 z\"{}/></marker>\n",
                    marker_id, marker_fill_attr
                ));
            }

            let d_path: String;
            let mx: f64;
            let my: f64;
            let tan_x: f64;
            let tan_y: f64;
            let (mut nsx, mut nsy, mut ntx, mut nty) = (sx, sy, tx, ty);

            if a.src == a.tgt {
                if a.loop_dir == "bottom" {
                    d_path = format!(
                        "M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}",
                        sx - 8.0,
                        sy + hh_s,
                        sx - 28.0,
                        sy + hh_s + 40.0,
                        sx + 28.0,
                        sy + hh_s + 40.0,
                        sx + 8.0,
                        sy + hh_s + 2.0
                    );
                    mx = sx;
                    my = sy + hh_s + 30.0;
                    tan_x = 1.0;
                    tan_y = 0.0;
                    a_label_pos = "below".to_string();
                    bound_min_x = bound_min_x.min(sx - 28.0);
                    bound_max_x = bound_max_x.max(sx + 28.0);
                    bound_max_y = bound_max_y.max(sy + hh_s + 40.0);
                } else if a.loop_dir == "left" {
                    d_path = format!(
                        "M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}",
                        sx - hw_s,
                        sy - 8.0,
                        sx - hw_s - 42.0,
                        sy - 26.0,
                        sx - hw_s - 42.0,
                        sy + 26.0,
                        sx - hw_s - 2.0,
                        sy + 8.0
                    );
                    mx = sx - hw_s - 31.0;
                    my = sy;
                    tan_x = 0.0;
                    tan_y = 1.0;
                    bound_min_x = bound_min_x.min(sx - hw_s - 42.0);
                    bound_min_y = bound_min_y.min(sy - 26.0);
                    bound_max_y = bound_max_y.max(sy + 26.0);
                } else if a.loop_dir == "right" {
                    d_path = format!(
                        "M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}",
                        sx + hw_s,
                        sy - 8.0,
                        sx + hw_s + 42.0,
                        sy - 26.0,
                        sx + hw_s + 42.0,
                        sy + 26.0,
                        sx + hw_s + 2.0,
                        sy + 8.0
                    );
                    mx = sx + hw_s + 31.0;
                    my = sy;
                    tan_x = 0.0;
                    tan_y = 1.0;
                    a_label_pos = "below".to_string();
                    bound_max_x = bound_max_x.max(sx + hw_s + 42.0);
                    bound_min_y = bound_min_y.min(sy - 26.0);
                    bound_max_y = bound_max_y.max(sy + 26.0);
                } else {
                    d_path = format!(
                        "M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}",
                        sx - 8.0,
                        sy - hh_s,
                        sx - 28.0,
                        sy - hh_s - 40.0,
                        sx + 28.0,
                        sy - hh_s - 40.0,
                        sx + 8.0,
                        sy - hh_s - 2.0
                    );
                    mx = sx;
                    my = sy - hh_s - 30.0;
                    tan_x = 1.0;
                    tan_y = 0.0;
                    bound_min_x = bound_min_x.min(sx - 28.0);
                    bound_max_x = bound_max_x.max(sx + 28.0);
                    bound_min_y = bound_min_y.min(sy - hh_s - 40.0);
                }
            } else {
                let dx = tx - sx;
                let dy = ty - sy;
                let mut chord_len = (dx * dx + dy * dy).sqrt();
                if chord_len == 0.0 {
                    chord_len = 1.0;
                }
                let sign_dx = if dx < 0.0 {
                    -1.0
                } else if dx > 0.0 {
                    1.0
                } else {
                    0.0
                };
                let sign_dy = if dy < 0.0 {
                    -1.0
                } else if dy > 0.0 {
                    1.0
                } else {
                    0.0
                };

                let (cx, cy) = if a.has_coords {
                    (
                        (a.ax - min_x) * scale_x + padding_x,
                        (max_y - a.ay) * scale_y + padding_y,
                    )
                } else if a_format == "curly" {
                    let mut curve_bulge = (chord_len * a_angle) / 100.0;
                    if auto_curved && curve_bulge.abs() > 70.0 {
                        curve_bulge = curve_bulge.signum() * 70.0;
                    }
                    (
                        (sx + tx) / 2.0 - (dy / chord_len) * curve_bulge,
                        (sy + ty) / 2.0 + (dx / chord_len) * curve_bulge,
                    )
                } else {
                    ((sx + tx) / 2.0, (sy + ty) / 2.0)
                };

                if a_format != "normal" || a.has_coords {
                    let dx_s = cx - sx;
                    let dy_s = cy - sy;
                    let mut l_s = (dx_s * dx_s + dy_s * dy_s).sqrt();
                    if l_s == 0.0 {
                        l_s = 1.0;
                    }
                    let dx_t = tx - cx;
                    let dy_t = ty - cy;
                    let mut l_t = (dx_t * dx_t + dy_t * dy_t).sqrt();
                    if l_t == 0.0 {
                        l_t = 1.0;
                    }

                    let abs_dx_s = dx_s.abs();
                    let abs_dy_s = dy_s.abs();
                    let dist_s = if abs_dx_s > 0.0 || abs_dy_s > 0.0 {
                        if abs_dx_s * hh_s > abs_dy_s * hw_s {
                            if abs_dx_s > 0.0 {
                                hw_s * l_s / abs_dx_s
                            } else {
                                0.0
                            }
                        } else if abs_dy_s > 0.0 {
                            hh_s * l_s / abs_dy_s
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    };

                    let abs_dx_t = dx_t.abs();
                    let abs_dy_t = dy_t.abs();
                    let dist_t = if abs_dx_t > 0.0 || abs_dy_t > 0.0 {
                        if abs_dx_t * hh_t > abs_dy_t * hw_t {
                            if abs_dx_t > 0.0 {
                                hw_t * l_t / abs_dx_t
                            } else {
                                0.0
                            }
                        } else if abs_dy_t > 0.0 {
                            hh_t * l_t / abs_dy_t
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    };

                    let extra_s = if !a.size_explicit && a.a_type != "2-arrow" && src_no_border {
                        4.0
                    } else {
                        l_s * (1.0 - a.size) / 2.0
                    };
                    let extra_t = if !a.size_explicit && a.a_type != "2-arrow" && tgt_no_border {
                        4.0
                    } else {
                        l_t * (1.0 - a.size) / 2.0
                    };

                    let gap_s = (dist_s + extra_s).min(l_s - 2.0);
                    let gap_t = (dist_t + extra_t).min(l_t - 2.0);

                    nsx = sx + dx_s * gap_s / l_s;
                    nsy = sy + dy_s * gap_s / l_s;
                    ntx = tx - dx_t * gap_t / l_t;
                    nty = ty - dy_t * gap_t / l_t;

                    if a_format == "curly" || a.has_coords {
                        d_path = format!(
                            "M {:.1} {:.1} Q {:.1} {:.1} {:.1} {:.1}",
                            nsx, nsy, cx, cy, ntx, nty
                        );
                        mx = (nsx + 2.0 * cx + ntx) / 4.0;
                        my = (nsy + 2.0 * cy + nty) / 4.0;
                        tan_x = ntx - nsx;
                        tan_y = nty - nsy;
                        bound_min_x = bound_min_x.min(mx);
                        bound_max_x = bound_max_x.max(mx);
                        bound_min_y = bound_min_y.min(my);
                        bound_max_y = bound_max_y.max(my);
                    } else {
                        d_path = format!("M {:.1} {:.1} L {:.1} {:.1}", nsx, nsy, ntx, nty);
                        mx = (nsx + ntx) / 2.0;
                        my = (nsy + nty) / 2.0;
                        tan_x = ntx - nsx;
                        tan_y = nty - nsy;
                    }
                } else {
                    if (sy - ty).abs() < 1e-6 {
                        let esx = sx + sign_dx * hw_s;
                        let etx = tx - sign_dx * hw_t;
                        let l_orth = (etx - esx).abs();
                        let gap_s = if !a.size_explicit && a.a_type != "2-arrow" && src_no_border {
                            4.0
                        } else {
                            l_orth * (1.0 - a.size) / 2.0
                        };
                        let gap_t = if !a.size_explicit && a.a_type != "2-arrow" && tgt_no_border {
                            4.0
                        } else {
                            l_orth * (1.0 - a.size) / 2.0
                        };
                        nsx = esx + sign_dx * gap_s;
                        ntx = etx - sign_dx * gap_t;
                        nsy = sy;
                        nty = ty;
                        d_path = format!("M {:.1} {:.1} L {:.1} {:.1}", nsx, sy, ntx, ty);
                        tan_x = ntx - nsx;
                        tan_y = 0.0;
                    } else if (sx - tx).abs() < 1e-6 {
                        let esy = sy + sign_dy * hh_s;
                        let ety = ty - sign_dy * hh_t;
                        let l_orth = (ety - esy).abs();
                        let gap_s = if !a.size_explicit && a.a_type != "2-arrow" && src_no_border {
                            4.0
                        } else {
                            l_orth * (1.0 - a.size) / 2.0
                        };
                        let gap_t = if !a.size_explicit && a.a_type != "2-arrow" && tgt_no_border {
                            4.0
                        } else {
                            l_orth * (1.0 - a.size) / 2.0
                        };
                        nsy = esy + sign_dy * gap_s;
                        nty = ety - sign_dy * gap_t;
                        nsx = sx;
                        ntx = tx;
                        d_path = format!("M {:.1} {:.1} L {:.1} {:.1}", sx, nsy, tx, nty);
                        tan_x = 0.0;
                        tan_y = nty - nsy;
                    } else {
                        let esx = sx + sign_dx * hw_s;
                        let ety = ty - sign_dy * hh_t;
                        let abs_tx_esx = (tx - esx).abs();
                        let abs_ety_sy = (ety - sy).abs();
                        let l_orth = abs_tx_esx + abs_ety_sy;
                        let gap_s = if !a.size_explicit && a.a_type != "2-arrow" && src_no_border {
                            4.0
                        } else {
                            l_orth * (1.0 - a.size) / 2.0
                        };
                        let gap_t = if !a.size_explicit && a.a_type != "2-arrow" && tgt_no_border {
                            4.0
                        } else {
                            l_orth * (1.0 - a.size) / 2.0
                        };
                        if gap_s <= abs_tx_esx {
                            nsx = esx + sign_dx * gap_s;
                            nsy = sy;
                        } else {
                            nsx = tx;
                            nsy = sy + sign_dy * (gap_s - abs_tx_esx);
                        }
                        if gap_t <= abs_ety_sy {
                            ntx = tx;
                            nty = ety - sign_dy * gap_t;
                        } else {
                            ntx = tx - sign_dx * (gap_t - abs_ety_sy);
                            nty = sy;
                        }
                        let nsy_eq_sy = (nsy - sy).abs() < 1e-6;
                        let ntx_eq_tx = (ntx - tx).abs() < 1e-6;
                        if (nsy_eq_sy && ntx_eq_tx) || (!nsy_eq_sy && !ntx_eq_tx) {
                            d_path = format!(
                                "M {:.1} {:.1} L {:.1} {:.1} L {:.1} {:.1}",
                                nsx, nsy, tx, sy, ntx, nty
                            );
                        } else {
                            d_path = format!("M {:.1} {:.1} L {:.1} {:.1}", nsx, nsy, ntx, nty);
                        }
                        tan_x = ntx - nsx;
                        tan_y = nty - nsy;
                    }
                    mx = (nsx + ntx) / 2.0;
                    my = (nsy + nty) / 2.0;
                }
            }

            terms_x.insert(a.name.clone(), mx);
            terms_y.insert(a.name.clone(), my);
            terms_w.insert(a.name.clone(), 14.0);
            terms_h.insert(a.name.clone(), 14.0);

            let path_props = match a.style.as_str() {
                "dashed" => " stroke-dasharray=\"6,6\"".to_string(),
                "dotted" => " stroke-dasharray=\"0,7\" stroke-linecap=\"round\"".to_string(),
                "wavy" => format!(" filter=\"url(#{})\"", wavy_id),
                _ => String::new(),
            };

            if a.a_type == "2-arrow" {
                let mut d_shaft = d_path.clone();
                if a.src != a.tgt && a_format != "curly" && !a.has_coords {
                    let s_dx = ntx - nsx;
                    let s_dy = nty - nsy;
                    let s_len = (s_dx * s_dx + s_dy * s_dy).sqrt();
                    if s_len > 6.0 {
                        let stx = ntx - (s_dx / s_len) * 2.5;
                        let sty = nty - (s_dy / s_len) * 2.5;
                        d_shaft = format!("M {:.1} {:.1} L {:.1} {:.1}", nsx, nsy, stx, sty);
                    }
                }
                layer_arrows.push_str(&format!(
                    "<path id=\"{}\" d=\"{}\" fill=\"none\"{}{}{} stroke-width=\"3\" />\n",
                    path_id, d_shaft, path_class_attr, path_color_attr, path_props
                ));
                layer_arrows.push_str(&format!(
                    "<path d=\"{}\" fill=\"none\" class=\"{}-bg-stroke\" stroke-width=\"1.5\"{} />\n",
                    d_shaft, cp, path_props
                ));
            } else {
                layer_arrows.push_str(&format!(
                    "<path id=\"{}\" d=\"{}\" fill=\"none\"{}{}{} stroke-width=\"1.5\" />\n",
                    path_id, d_path, path_class_attr, path_color_attr, path_props
                ));
            }
            layer_arrows.push_str(&format!(
                "<path d=\"{}\" fill=\"none\" stroke=\"none\" marker-end=\"url(#{})\" />\n",
                d_path, marker_id
            ));

            if !a.label_raw.is_empty() {
                let mut tan_len = (tan_x * tan_x + tan_y * tan_y).sqrt();
                if tan_len == 0.0 {
                    tan_len = 1.0;
                }
                let ux = tan_x / tan_len;
                let uy = tan_y / tan_len;
                let mut nx = -uy;
                let mut ny = ux;
                if ny > 0.0 || (ny == 0.0 && nx > 0.0) {
                    nx = -nx;
                    ny = -ny;
                }
                let dist_lbl = 15.0;
                let (mut lx, ly) = if a_label_pos == "below" {
                    (mx - dist_lbl * nx, my - dist_lbl * ny)
                } else {
                    (mx + dist_lbl * nx, my + dist_lbl * ny)
                };

                let mut txt_anchor = "middle";
                if ny.abs() < 0.5 {
                    if lx > mx {
                        txt_anchor = "start";
                        lx = mx + 10.0;
                    } else {
                        txt_anchor = "end";
                        lx = mx - 10.0;
                    }
                }

                let lbl_w = a.label_w.max(32.0);
                let lbl_h = a.label_h.max(18.0);
                let (lbl_x1, lbl_x2) = match txt_anchor {
                    "start" => (lx, lx + lbl_w),
                    "end" => (lx - lbl_w, lx),
                    _ => (lx - lbl_w / 2.0, lx + lbl_w / 2.0),
                };
                let lbl_y1 = ly - lbl_h / 2.0;
                let lbl_y2 = ly + lbl_h / 2.0;

                bound_min_x = bound_min_x.min(lbl_x1);
                bound_max_x = bound_max_x.max(lbl_x2);
                bound_min_y = bound_min_y.min(lbl_y1);
                bound_max_y = bound_max_y.max(lbl_y2);

                if a.has_tex {
                    let html_br = a.label_html.replace("\\n", "<br/>");
                    let fo_class = if a.a_class.is_empty() {
                        format!("{}-label-tex", cp)
                    } else {
                        format!("{}-label-tex {}", cp, a.a_class)
                    };
                    let flex_align = match txt_anchor {
                        "start" => "flex-start",
                        "end" => "flex-end",
                        _ => "center",
                    };
                    let text_align = match txt_anchor {
                        "start" => "left",
                        "end" => "right",
                        _ => "center",
                    };
                    layer_labels.push_str(&format!(
                        "<foreignObject x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" style=\"overflow: visible;\"><div xmlns=\"http://www.w3.org/1999/xhtml\" class=\"{}\" style=\"display: inline-flex; flex-direction: column; align-items: {}; justify-content: center; text-align: {}; width: 100%; height: 100%; background: var(--bg, #ffffff); border-radius: 3px; padding: 1px 4px; font-family: var(--family-diagram, monospace); font-size: 14px; pointer-events: none;\">{}</div></foreignObject>\n",
                        lbl_x1, lbl_y1, lbl_w, lbl_h, fo_class, flex_align, text_align, html_br
                    ));
                } else {
                    let l_arr: Vec<&str> = a.label_raw.split("\\n").collect();
                    if l_arr.len() <= 1 {
                        layer_labels.push_str(&format!(
                            "<text x=\"{}\" y=\"{}\" text-anchor=\"{}\" dominant-baseline=\"central\"{}{}>{}</text>\n",
                            lx, ly, txt_anchor, label_class_attr, label_color_attr, html_escape(&a.label_raw)
                        ));
                    } else {
                        let line_height = 18.0;
                        let start_dy = -((l_arr.len() - 1) as f64) * line_height / 2.0;
                        let mut tspans = String::new();
                        for (idx_l, part) in l_arr.iter().enumerate() {
                            let ldy = if idx_l == 0 { start_dy } else { line_height };
                            tspans.push_str(&format!(
                                "<tspan x=\"{}\" dy=\"{}\">{}</tspan>",
                                lx,
                                ldy,
                                html_escape(part)
                            ));
                        }
                        layer_labels.push_str(&format!(
                            "<text x=\"{}\" y=\"{}\" text-anchor=\"{}\" dominant-baseline=\"central\"{}{}>{}</text>\n",
                            lx, ly, txt_anchor, label_class_attr, label_color_attr, tspans
                        ));
                    }
                }
            }
        }
    }

    let mut layer_nodes = String::new();
    let cp = cls_prefix;
    for n in &nodes {
        let tx = terms_x[&n.name];
        let ty = terms_y[&n.name];
        let w = terms_w[&n.name];
        let h = terms_h[&n.name];
        let bx = tx - w / 2.0;
        let by = ty - h / 2.0;

        let is_borderless = n.border == "none" || n.border.is_empty();
        let has_custom_bg = !n.bg.is_empty() && n.bg != "default" && n.bg != "none";

        if !is_borderless || has_custom_bg {
            let mut rx = 18;
            if n.border == "squared" {
                rx = 4;
            }
            if is_borderless {
                rx = 0;
            }
            let mut inline_style = String::new();
            if n.bg == "none" || (is_borderless && !has_custom_bg) {
                inline_style.push_str("fill: none !important; ");
            } else if has_custom_bg {
                inline_style.push_str(&format!("fill: {} !important; ", n.bg));
            }
            if is_borderless {
                inline_style.push_str("stroke: none !important; border: none !important; ");
            }
            let style_attr = if inline_style.is_empty() {
                String::new()
            } else {
                format!(" style=\"{}\"", inline_style)
            };
            let n_cls = if n.n_class.is_empty() {
                format!("{}-node", cp)
            } else {
                format!("{}-node {}", cp, n.n_class)
            };
            let sw = if is_borderless { "0" } else { "1.5" };
            layer_nodes.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" class=\"{}\" stroke-width=\"{}\"{} />\n",
                bx, by, w, h, rx, n_cls, sw, style_attr
            ));
        }

        if n.has_tex {
            let html_br = n.html_text.replace("\\n", "<br/>");
            layer_nodes.push_str(&format!(
                "<foreignObject x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"><div xmlns=\"http://www.w3.org/1999/xhtml\" style=\"display: flex; flex-direction: column; align-items: center; justify-content: center; width: 100%; height: 100%; text-align: center; font-family: var(--family-diagram, monospace); font-size: 14px; pointer-events: none;\">{}</div></foreignObject>\n",
                bx, by, w, h, html_br
            ));
        } else {
            let n_cls = if n.n_class.is_empty() {
                format!("{}-text", cp)
            } else {
                format!("{}-text {}", cp, n.n_class)
            };
            let l_arr: Vec<&str> = n.raw_text.split("\\n").collect();
            if l_arr.len() <= 1 {
                layer_nodes.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" dominant-baseline=\"central\" class=\"{}\"><tspan x=\"{}\" dy=\"0\">{}</tspan></text>\n",
                    tx,
                    ty,
                    n_cls,
                    tx,
                    html_escape(&n.raw_text)
                ));
            } else {
                let line_height = 18.0;
                let start_dy = -((l_arr.len() - 1) as f64) * line_height / 2.0;
                let mut tspans = String::new();
                for (idx_l, part) in l_arr.iter().enumerate() {
                    let ldy = if idx_l == 0 { start_dy } else { line_height };
                    tspans.push_str(&format!(
                        "<tspan x=\"{}\" dy=\"{}\">{}</tspan>",
                        tx,
                        ldy,
                        html_escape(part)
                    ));
                }
                layer_nodes.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" dominant-baseline=\"central\" class=\"{}\">{}</text>\n",
                    tx, ty, n_cls, tspans
                ));
            }
        }
    }

    let margin = 4.0;
    bound_min_x -= margin;
    bound_max_x += margin;
    bound_min_y -= margin;
    bound_max_y += margin;
    let final_w = (bound_max_x - bound_min_x).max(1.0);
    let final_h = (bound_max_y - bound_min_y).max(1.0);

    let mut out = String::with_capacity(2048);
    out.push_str(&format!(
        "<div id=\"{}\" class=\"{}-container\" style=\"overflow-x: auto;\">\n",
        diagram_id, cp
    ));
    out.push_str(&format!(
        "<svg role=\"img\" aria-label=\"{}\" width=\"{:.0}\" height=\"{:.0}\" viewBox=\"{:.0} {:.0} {:.0} {:.0}\" xmlns=\"http://www.w3.org/2000/svg\" style=\"max-width: none; display: block;\">\n",
        aria_label, final_w, final_h, bound_min_x, bound_min_y, final_w, final_h
    ));
    out.push_str("<style>\n");
    out.push_str(&format!(
        "    #{} .{}-node {{ fill: var(--bg-code, #ffffff); stroke: var(--gray4, #333333); }}\n",
        diagram_id, cp
    ));
    out.push_str(&format!(
        "    #{} .{}-text, #{} .{}-text * {{ fill: var(--fg, #333333); font-family: var(--family-diagram, monospace) !important; font-size: 14px; }}\n",
        diagram_id, cp, diagram_id, cp
    ));
    out.push_str(&format!(
        "    #{} .{}-label, #{} .{}-label * {{ paint-order: stroke fill; stroke: var(--bg-code, #ffffff); stroke-width: 4px; stroke-linejoin: round; }}\n",
        diagram_id, cp, diagram_id, cp
    ));
    out.push_str(&format!(
        "    #{} .{}-path {{ stroke: var(--gray4, #333333); }}\n",
        diagram_id, cp
    ));
    out.push_str(&format!(
        "    #{} .{}-bg-stroke {{ stroke: var(--bg, #ffffff); }}\n",
        diagram_id, cp
    ));
    out.push_str(&format!(
        "    #{} .{}-marker {{ fill: var(--gray4, #333333); }}\n",
        diagram_id, cp
    ));
    out.push_str(&format!(
        "    #{} text, #{} tspan {{ font-family: var(--family-diagram, monospace) !important; font-size: 14px !important; }}\n",
        diagram_id, diagram_id
    ));
    out.push_str(&format!(
        "    #{} .latex-inline svg {{ vertical-align: middle; }}\n",
        diagram_id
    ));
    out.push_str("</style>\n");
    out.push_str(&defs_html);
    out.push('\n');
    out.push_str(&layer_arrows);
    out.push_str(&layer_labels);
    out.push_str(&layer_nodes);
    out.push_str("</svg>");

    if !safe_caption.is_empty() {
        out.push_str(&format!(
            "\n<p><a href=\"#{}\" class=\"{}-caption\">{}</a></p>",
            diagram_id, cp, safe_caption
        ));
    }
    out.push_str("\n</div>\n");

    state
        .list_html_map
        .entry("diagram".to_string())
        .or_default()
        .push_str(&format!(
            "<li><a href=\"#{}\">Diagram {}</a></li>\n",
            diagram_id, state.page_diagram_count
        ));

    out
}
