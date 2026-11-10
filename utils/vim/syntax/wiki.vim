let s:current_dir = expand('<sfile>:p:h')

if hlexists('wikiCode')
    syntax clear wikiCode
endif
if hlexists('wikiCodeBlock')
    syntax clear wikiCodeBlock
endif

" Load built-in LaTeX syntax into @wikiTex
let s:save_csyntax = exists('b:current_syntax') ? b:current_syntax : ''
unlet! b:current_syntax
silent! syntax include @wikiTex syntax/tex.vim
if !empty(s:save_csyntax)
    let b:current_syntax = s:save_csyntax
endif

" Cluster of code/LaTeX regions that should NOT be overridden by general wiki matches
syntax cluster wikiNoOverride contains=wikiCode,wikiPreBlock,wikiCodeBlockBody,wikiTexInline,wikiTexDisplay,wikiTexBlockBody,@wikiTex

" italic: "_<text>_"
syntax region wikiItalic matchgroup=wikiItalicDelim start=/_/ skip=/\\_/ end=/_/ concealends

" bold: "__<text>__"
syntax region wikiBold matchgroup=wikiBoldDelim start=/__/ skip=/\\_/ end=/__/ concealends

" delimiters: "(", ")", "'"
syntax match wikiDelimiter /[()"']/ containedin=ALLBUT,@wikiNoOverride

" preformatted: "`<text>`" and "<pre>...</pre>"
syntax region wikiCode matchgroup=wikiCodeDelim start=/`\@<!`\(`\)\@!/ skip=/\\`/ end=/`\@<!`\(`\)\@!/ concealends contains=@NoSpell
syntax region wikiPreBlock matchgroup=wikiHtmlBlock start=/<pre>/ end=/<\/pre>/ contains=@NoSpell

" LaTeX inline: "$ ... $"
syntax region wikiTexInline
    \ matchgroup=wikiTexDelim
    \ start=/\\\@<!\$\$\@!/
    \ skip=/\\\$/
    \ end=/\\\@<!\$/
    \ contains=@wikiTex,@NoSpell
    \ keepend

" LaTeX display: "$$ ... $$"
syntax region wikiTexDisplay
    \ matchgroup=wikiTexDelim
    \ start=/\\\@<!\$\$/
    \ end=/\\\@<!\$\$/
    \ contains=@wikiTex,@NoSpell
    \ keepend extend

" marked: "==<text>=="
syntax match wikiMarkFull /==.\{-}==/ contains=wikiMarkDelim,wikiMarkValue
syntax match wikiMarkDelim /==/ contained
syntax match wikiMarkValue /\(==\)\@<=.\{-}\ze==/ contained

" underline: "++<text>++"
syntax match wikiUnderlineFull /++.\{-}++/ contains=wikiUnderlineDelim,wikiUnderlineValue
syntax match wikiUnderlineDelim /++/ contained conceal
syntax match wikiUnderlineValue /\(++\)\@<=.\{-}\ze++/ contained

" link: "[<label>](<url>)"
syntax match wikiLinkFull /\[.\{-}\](.\{-})/ contains=wikiLinkText,wikiLinkUrl
syntax region wikiLinkText matchgroup=Comment start=/\[/ end=/\]/ contained nextgroup=wikiLinkUrl
syntax region wikiLinkUrl matchgroup=Comment start=/(/ end=/)/ contained

" title: "# <title>"
syntax match wikiTitleDelimiter /^\s*#\+/ nextgroup=wikiTitleSpaces
syntax match wikiTitleSpaces /\s\+/ contained nextgroup=wikiTitleText
syntax match wikiTitleText /.*$/ contained

" lists: "<n>. <something>" and "- <something>"
syntax match wikiListMarker /^\s*\zs\(-\)\ze\s\+/
syntax match wikiOrderedListMarker /^\s*\zs\d\+[.)]\ze\s\+/

" line: ":: <entry>: <arg>=xxx, <arg>=yyy"
syntax match wikiRefsLine /^\s*::\s*__refs__:.*$/ containedin=ALLBUT,@wikiNoOverride contains=wikiKeyword,@NoSpell
syntax match wikiColons /^\s*::\s*/ containedin=ALLBUT,@wikiNoOverride contains=wikiRefsLine nextgroup=wikiKeyword
syntax match wikiKeyword /[a-zA-Z0-9_-]\+:/ contained
syntax match wikiArgName /\(^\s*::.*\)\@<=\<[a-zA-Z0-9_-]\+\ze=/ containedin=ALLBUT,@wikiNoOverride nextgroup=wikiArgEquals
syntax match wikiArgEquals /=/ contained nextgroup=wikiArgValue,wikiArgString
syntax match wikiArgValue /[^, "]\+/ contained
syntax region wikiArgString start=/"/ skip=/\\"/ end=/"/ contained
syntax match wikiArgSeparator /\(^\s*::.*\)\@<=,/ containedin=ALLBUT,@wikiNoOverride

" blocks: "{{ block.<name> }}"
syntax match wikiBlockBraces /{{\|}}/ contained
syntax match wikiBlockWord /[a-zA-Z0-9_-]\+/ contained
syntax match wikiBlockNamespace /[a-zA-Z0-9_-]\+\ze\./ contained
syntax match wikiBlockDot /\./ contained
syntax match wikiBlockName /\(\.\)\@<=[a-zA-Z0-9_-]\+/ contained

" html: "<tag class='...'>...</tag>"
syntax region wikiCustomHtmlTag matchgroup=wikiHtmlBlock start=/<\/\?/ end=/>/ containedin=ALLBUT,@wikiNoOverride oneline contains=wikiHtmlTagName,wikiHtmlArgName,wikiHtmlArgEquals,wikiHtmlArgString
syntax match wikiHtmlTagName /\(<\/\?\)\@<=[a-zA-Z0-9_-]\+/ contained
syntax match wikiHtmlArgName /[a-zA-Z0-9_-]\+\ze=/ contained
syntax match wikiHtmlArgEquals /=/ contained
syntax region wikiHtmlArgString start=/"/ skip=/\\"/ end=/"/ contained

" refs: "{{ <topic>:<context>:<label> }}"
syntax match wikiRefDelimStart /{{\ze[^:]\+:[^:]\+:[^}]\+}}/ containedin=ALLBUT,@wikiNoOverride conceal cchar=" nextgroup=wikiRefTopic
syntax match wikiRefTopic /[^:]\+/ contained conceal nextgroup=wikiRefColon1
syntax match wikiRefColon1 /:/ contained conceal nextgroup=wikiRefContext
syntax match wikiRefContext /[^:]\+/ contained conceal nextgroup=wikiRefColon2
syntax match wikiRefColon2 /:/ contained conceal nextgroup=wikiRefLabel
syntax match wikiRefLabel /[^}]\+/ contained nextgroup=wikiRefDelimEnd
syntax match wikiRefDelimEnd /}}/ contained conceal cchar="

" numbers
syntax match wikiNumber /\<\d\+\([.,]\d\+\)\?\>\([.)]\s\)\@!/ containedin=ALLBUT,@wikiNoOverride

" vars: "{{ <var> }}"
silent! syntax clear wikiVar
syntax region wikiVar matchgroup=wikiBlockBraces start=/{{\ze[^:]*}}/ end=/}}/ keepend oneline containedin=ALLBUT,@wikiNoOverride contains=wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot

" block/tag arguments (e.g. lang=python)
syntax match wikiTagArgName /\<lang\ze=/ contained nextgroup=wikiTagArgEquals
syntax match wikiTagArgEquals /=/ contained nextgroup=wikiTagArgValue
syntax match wikiTagArgValue /[a-zA-Z0-9_-]\+/ contained

" block.plain
silent! syntax clear wikiPlainBlockStart
silent! syntax clear wikiPlainBlockBody
syntax match wikiPlainBlockStart /^\s*{{\s*block\.plain\s*}}/ contains=wikiBlockBraces,wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot nextgroup=wikiPlainBlockBody skipnl
syntax region wikiPlainBlockBody start=/^/ end=/^\s*\ze{{\s*block\.end\s*}}/ contained

" block.code (without lang=...): highlight body entirely as inline code (wikiCode)
silent! syntax clear wikiCodeBlockStart
silent! syntax clear wikiCodeBlockBody
syntax match wikiCodeBlockStart /^\s*{{\s*block\.code\s*}}\s*$/ contains=wikiBlockBraces,wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot nextgroup=wikiCodeBlockBody skipnl
syntax region wikiCodeBlockBody start=/^/ end=/^\s*\ze{{\s*block\.end\s*}}/ contained contains=@NoSpell

" block.tex: highlight body with LaTeX syntax
silent! syntax clear wikiTexBlockStart
silent! syntax clear wikiTexBlockBody
syntax match wikiTexBlockStart /^\s*{{\s*block\.tex\s*}}\s*$/ contains=wikiBlockBraces,wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot nextgroup=wikiTexBlockBody skipnl
syntax region wikiTexBlockBody start=/^/ end=/^\s*\ze{{\s*block\.end\s*}}/ contained contains=@wikiTex,@NoSpell keepend extend

" block.end
syntax match wikiBlockEnd /^\s*{{\s*block\.end\s*}}/ contains=wikiBlockBraces,wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot

" block.code with lang=<ft>
let s:filetypes = {
    \ 'python': 'python',
    \ 'js': 'javascript',
    \ 'javascript': 'javascript',
    \ 'html': 'html',
    \ 'css': 'css',
    \ 'sh': 'sh',
    \ 'bash': 'sh',
    \ 'json': 'json',
    \ 'sql': 'sql',
    \ 'php': 'php'
    \ }

for [s:marker, s:ft] in items(s:filetypes)
    if empty(globpath(&rtp, 'syntax/' . s:ft . '.vim', 1, 1))
        continue
    endif
    let s:save_csyntax = exists('b:current_syntax') ? b:current_syntax : ''
    unlet! b:current_syntax
    execute 'silent! syntax include @wikiCode_' . s:ft . ' syntax/' . s:ft . '.vim'
    if !empty(s:save_csyntax)
        let b:current_syntax = s:save_csyntax
    endif

    execute 'syntax match wikiBlockCodeStart_' . s:ft
        \ . ' /^\s*{{\s*block\.code\s\+lang=' . s:marker . '\s*}}\s*$/'
        \ . ' contains=wikiBlockBraces,wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot,wikiTagArgName,wikiTagArgEquals,wikiTagArgValue'
        \ . ' nextgroup=wikiBlockBody_' . s:ft . ' skipnl'

    execute 'syntax region wikiBlockBody_' . s:ft
        \ . ' start=/^/'
        \ . ' end=/^\s*\ze{{\s*block\.end\s*}}/'
        \ . ' contained keepend extend contains=@wikiCode_' . s:ft . ',@NoSpell'

    execute 'syntax cluster wikiNoOverride add=wikiBlockBody_' . s:ft . ',@wikiCode_' . s:ft
endfor


" HIGHLIGHT
hi wikiItalic ctermfg=2 cterm=italic,underline
hi link wikiItalicDelim Comment
hi wikiBold ctermfg=2 cterm=bold,underline
hi link wikiBoldDelim Comment
hi link wikiCodeDelim Comment
hi wikiCode ctermfg=13 cterm=underline
hi link wikiCodeBlock Code
hi link wikiCodeBlockBody wikiCode
hi link wikiTexDelim Comment

hi link wikiDelimiter Delimiter
hi link wikiNumber Constant

hi wikiLinkUrl ctermfg=4 cterm=underline
hi wikiLinkText ctermfg=5
hi wikiTitleDelimiter ctermfg=5
hi wikiTitleText ctermfg=12 cterm=underline
hi link wikiListMarker Statement
hi link wikiOrderedListMarker Statement

hi link wikiColons Comment
hi link wikiKeyword Delimiter
hi link wikiArgName Constant
hi link wikiArgEquals Operator
hi link wikiArgValue String
hi link wikiArgSeparator Operator
hi link wikiArgString String

hi link wikiBlockBraces Comment
hi wikiBlockWord Title
hi wikiBlockNamespace ctermfg=5
hi link wikiBlockDot Comment
hi link wikiBlockName Type

hi! link wikiPlainBlockBody Normal

hi link wikiHtmlBlock Comment
hi wikiHtmlTagName ctermfg=4
hi link wikiHtmlArgName Constant
hi link wikiHtmlArgEquals Operator
hi link wikiHtmlArgString String

" ==something==
hi link wikiMarkDelim Comment
hi link wikiMarkValue Highlight

" ++something++
hi link wikiUnderlineDelim Comment
hi link wikiUnderlineValue Underlined

" =foo=something==
hi link wikiTagDelim Comment
hi link wikiTagEnd Comment
hi link wikiTagLabel Constant
hi link wikiTagValue Special

" {{ <topic>:<context>:<label> }}
hi link wikiRefDelimStart Comment
hi link wikiRefDelimEnd Comment
hi link wikiRefColon1 Comment
hi link wikiRefColon2 Comment
hi link wikiRefTopic Type
hi link wikiRefContext Identifier
hi wikiRefLabel ctermfg=4 cterm=underline

hi! link wikiTagArgName Constant
hi! link wikiTagArgEquals Operator
hi! link wikiTagArgValue String

setlocal conceallevel=2
let b:current_syntax = 'wiki'
