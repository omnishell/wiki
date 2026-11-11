# Omni Wiki (`wiki`)

# About

**`wiki`** is a fast, full-featured static site and personal knowledge base generator built for the [Omni Shell Framework](../omni/README.md). It compiles `.md` and `.wiki` files into self-contained, interlinked HTML websites featuring incremental content-hash caching, parallel multi-job builds, server-side LaTeX math rendering (`bin/latex`), SVG node-edge diagram generation, structured knowledge blocks (`def`, `table`, `diagram`, `quote`, `code`, `tex`, `plain`), semantic cross-references, dynamic page/block listings and counters, client-side search indexing (`search_index.json`), automatic `sitemap.xml` generation, a built-in HTTP preview server (`bin/server`), and Vim syntax/autocompletion plugins (`utils/vim/`).

## Key Features

- **Incremental & Parallel Builds (`wiki build`)**: Tracks SHA-1 hashes of pages, templates, static files, and global references in `.cache/` to re-render only modified pages across configurable parallel worker jobs (`-j|--jobs`), or compile a single page on the fly (`wiki build <wiki>.<page>`).
- **Rich Academic & Technical Markup**:
  - Inline (`$...$`) and display (`$$...$$`) LaTeX math compiled via `bin/latex`.
  - Structured semantic blocks (`def` definitions with `term`, `context`, `idea`, `details`, `related`; ASCII `table` to HTML; `diagram` to auto-laid-out SVG; `quote`; `code`; `tex`; `plain`; plus custom CSS blocks).
  - Anchors (`{{#id}}`), numbered sequential occurrences (`{{=id}}`), external topic/reference citations (`{{<topic>:<ref>:<label>}}`), and internal page links (`{{page:<slug>:<label>}}`).
- **Dynamic Queries (`list.*` & `count.*`)**: Embed filtered, sorted, paginated lists or counts of pages and semantic blocks directly inside any page or template.
- **Customizable Layouts & Partials**: Full HTML template engine with automatic navigation menu (`wiki.menu`), breadcrumbs (`wiki.breadcrumb`), table of contents (`toc`), search modal (`wiki.search`), theme/font settings (`wiki.settings`), tags (`wiki.tags`), and related pages (`wiki.related`).
- **Vim Integration (`utils/vim/`)**: Syntax highlighting (`syntax/wiki.vim`) and `<C-x><C-o>` omni-completion (`filetype/wiki.vim`) for blocks, listings, partials, page slugs, and reference keys.

---

# Install

## Via the Official `omni` Provider

```bash
omni providers install omni
omni tools use wiki --provider omni
```

## From a Local Directory

```bash
omni tools use -p /path/to/omni/wiki
```

## Directly from GitHub

```bash
omni tools use -u https://github.com/omnishell/wiki.git
```

## Optional: Vim Syntax & Autocompletion Plugin

To enable `.wiki` filetype detection, syntax highlighting, and `<C-x><C-o>` omni-completion in Vim/Neovim:

```bash
mkdir -p ~/.vim/syntax ~/.vim/ftplugin
ln -sf /path/to/omni/wiki/utils/vim/syntax/wiki.vim ~/.vim/syntax/wiki.vim
ln -sf /path/to/omni/wiki/utils/vim/filetype/wiki.vim ~/.vim/ftplugin/wiki.vim
```

---

# Usage

## 1. Managing Wikis (`wiki new|set|ls|rm`)

```bash
# List all registered wikis
wiki ls

# Create or register a wiki
wiki new|set <wiki_name> -p|--path <root_path> \
  [-c|--contents <contents_dir>] \
  [-d|--dist <dist_dir>] \
  [-t|--templates <templates_dir>] \
  [-s|--static <static_dir>] \
  [-a|--assets <assets_dir>] \
  [-D|--domain <https://example.com>]

# Unregister a wiki
wiki rm <wiki_name>
```

By default, creating a new wiki initializes the directory layout (`contents/`, `dist/`, `templates/`, `static/`, `assets/`) and copies the default theme assets and templates.

---

## 2. Building & Serving (`wiki build` & `wiki serve`)

```bash
# Incremental build of an entire wiki (defaults to parallel jobs = CPU core count)
wiki build <wiki_name> [-j|--jobs <N>]

# Force a full rebuild ignoring cache
wiki build <wiki_name> --rebuild

# Build a single page within a wiki (e.g., contents/math/algebra.md -> mywiki.math.algebra)
wiki build <wiki_name>.<page_slug>

# Build using custom directory overrides
wiki build [<wiki_name>] \
  [-c|--contents <dir>] \
  [-d|--dist <dir>] \
  [-t|--templates <dir>] \
  [-s|--static <dir>] \
  [-a|--assets <dir>] \
  [-C|--cache <dir>] \
  [-j|--jobs <N>]

# Serve the built wiki locally (uses bin/server with netcat fallback; default port: 8080)
wiki serve <wiki_name> [-p|--port <port>]
```

---

## 3. Managing Pages, Templates, Topics, References, Blocks & Authors

### Pages (`wiki pages`)

```bash
# List all pages in a wiki (dot-separated slugs)
wiki pages ls <wiki_name>

# Create a new page (optionally initialized from a registered template)
wiki pages new|set <wiki_name> -p|--page <relative/path.md> [-t|--template <template_name>]

# Delete a page from a wiki (supports dot-slug notation like `notes.intro`)
wiki pages rm <wiki_name> -p|--page <page_slug>
```

### Page Templates (`wiki template`)

```bash
wiki template ls
wiki template new|set <template_name> -p|--path <path/to/template.md>
wiki template rm <template_name>
```

### Topics & External References (`wiki topic` & `wiki ref`)

Topics group external reference URLs (e.g., `wiki`, `arxiv`, `doi`, `rfc`) that can be cited in pages using `{{<topic>:<ref>:<label>}}`:

```bash
# Manage topics
wiki topic ls
wiki topic new|set <topic> -w|--wiki <wiki_name> [-e|--env <env>]
wiki topic rm <topic>

# Manage reference keys and target URLs under a topic
wiki ref ls
wiki ref new|set <ref_key> -w|--wiki <wiki_name> [-e|--env <env>] -t|--topic <topic> -v|--value <url>
wiki ref rm <ref_key>
```

### Custom CSS Blocks (`wiki block`)

Register custom block names mapped to HTML/CSS classes:

```bash
wiki block ls
wiki block new|set <block_name> -c|--class <css_class>
wiki block rm <block_name>
```

### Authors (`wiki author`)

Register author metadata (`name` and `url`) referenced by `author: <id>` in page frontmatter:

```bash
wiki author ls
wiki author new|set <author_id> -w|--wiki <wiki_name> -n|--name "<Full Name>" -u|--url "<https://...>"
wiki author rm <author_id>
```

---

## 4. Markup & Syntax Reference

### Page Frontmatter (`---` or `{{ block.globals }}`)

Define page metadata at the top of a `.md` or `.wiki` file:

```yaml
---
title: Linear Algebra Notes
weight: 10
tags: math, linear-algebra, matrices
thumb: assets/matrix.png
author: alice
related: math/calculus.md, math/spectral.md
partials: menu, breadcrumb, search, settings, help
---
```

### Inline Formatting & Math

| Syntax | Output |
| :--- | :--- |
| `__bold__` or `**bold**` | **Bold** (`<strong>`) |
| `_italic_` | *Italic* (`<em>`) |
| `++underline++` | <ins>Underline</ins> (`<ins>`) |
| `` `inline code` `` | `<code>inline code</code>` |
| `$E = mc^2$` | Inline LaTeX math |
| `$$ \int_{-\infty}^{\infty} e^{-x^2} dx = \sqrt{\pi} $$` | Display LaTeX math block |
| `[Link Text](other-page.md)` | Link (`.md` / `.wiki` extensions are automatically converted to `.html`) |
| `{{page:notes.intro:Introduction}}` | Cross-link to wiki page slug `notes.intro` with label `Introduction` |
| `{{<topic>:<ref>:<label>}}` | External reference link configured via `wiki ref` |
| `{{#thm-1}}` | Create named anchor `#thm-1` |
| `{{=thm-1}}` | Auto-incremented numbered link (`1`, `2`, ...) pointing to `#thm-1` |

### Semantic Blocks (`{{ block.<type> }} ... {{ block.end }}`)

Blocks can be written as `{{ block.<type> [attrs] }} ... {{ block.end }}` or shorthand `{{<type>}} ... {{end}}`:

1. **Definition Block (`def`)**:
   ```wiki
   {{ block.def id=group }}
   :: term: Group
   :: context: Abstract Algebra
   :: idea: A set equipped with an associative binary operation, an identity element, and inverses.
   :: details:
   Formally, $(G, \cdot)$ satisfies closure, associativity, identity $e$, and inverse $g^{-1}$.
   :: related: Ring | ring.html, Field | field.html
   {{ block.end }}
   ```
2. **ASCII Table Block (`table`)**:
   ```wiki
   {{ block.table }}
   +--------+--------+
   | Header | Value  |
   +--------+--------+
   | Alpha  | 100    |
   +--------+--------+
   {{ block.end }}
   ```
3. **SVG Diagram Block (`diagram`)**:
   ```wiki
   {{ block.diagram }}
   :: node: [A] Source (top)
   :: node: [B] Transform (right)
   :: node: [C] Sink (bottom)
   :: arrow: [A] -> [B] : parse
   :: 2-arrow: [B] <-> [C] : sync
   {{ block.end }}
   ```
4. **Quote Block (`quote`)**:
   ```wiki
   {{ block.quote }}
   :: cite: Donald Knuth
   :: text: Premature optimization is the root of all evil.
   {{ block.end }}
   ```
5. **Code Block (`code`)**:
   ```wiki
   {{ block.code lang=python }}
   def fib(n):
       return n if n < 2 else fib(n - 1) + fib(n - 2)
   {{ block.end }}
   ```
6. **LaTeX Block (`tex`)** and **Plain Unparsed Block (`plain`)**:
   `{{ block.tex }} ... {{ block.end }}` and `{{ block.plain }} ... {{ block.end }}`.

### Dynamic Listings & Counters (`list.*` & `count.*`)

Query pages or blocks across the wiki:

```wiki
# List up to 10 pages in `notes/` tagged `math`, sorted by weight
{{ list.pages dir=notes tags=math sort=weight max=10 style=bullet date=true }}

# List all `def` blocks across the wiki in a 2-column box grid with pagination
{{ list.blocks def style=box cols=2 paginate=true max=12 sort=alpha }}

# Output total count of pages or specific blocks matching filters
{{ count.pages dir=notes tags=math }}
{{ count.blocks def dir=notes }}
```

### Layout & Page Partials

Available placeholders for HTML templates (`templates/layout.html`) and pages:

- **Page Content & Metadata**: `{{ page.body }}`, `{{ page.title }}`, `{{ page.name }}`, `{{ page.<frontmatter_key> }}`, `{{ toc }}`, `{{ files }}`
- **Wiki Components**: `{{ wiki.head }}`, `{{ wiki.menu }}`, `{{ wiki.featured }}`, `{{ wiki.breadcrumb }}`, `{{ wiki.search }}`, `{{ wiki.settings }}`, `{{ wiki.help }}`, `{{ wiki.author }}`, `{{ wiki.tags }}`, `{{ wiki.thumb }}`, `{{ wiki.related }}`
- **Date & Time**: `{{ datetime.year }}`, `{{ datetime.now }}`
