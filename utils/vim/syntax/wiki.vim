syntax clear

" Load built-in LaTeX syntax into @wikiTex
unlet! b:current_syntax
silent! syntax include @wikiTex syntax/tex.vim
unlet! b:current_syntax

" Load language syntaxes for {{ block.code lang=<ft> }}
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

let s:loaded_fts = {}
for [s:marker, s:ft] in items(s:filetypes)
    if !has_key(s:loaded_fts, s:ft) && !empty(globpath(&rtp, 'syntax/' . s:ft . '.vim', 1, 1))
        unlet! b:current_syntax
        execute 'silent! syntax include @wikiCode_' . s:ft . ' syntax/' . s:ft . '.vim'
        unlet! b:current_syntax
        let s:loaded_fts[s:ft] = 1
    endif
endfor

" Cluster of code/LaTeX regions that must NOT be overridden by general wiki matches
syntax cluster wikiNoOverride contains=wikiCode,wikiPreBlock,wikiCodeBlockBody,wikiTexInline,wikiTexDisplay,wikiTexBlockBody,wikiBlockHeader,@wikiTex,@texMathZoneGroup

" italic: "_<text>_"
syntax region wikiItalic matchgroup=wikiItalicDelim start=/_/ skip=/\\_/ end=/_/ oneline concealends

" bold: "__<text>__"
syntax region wikiBold matchgroup=wikiBoldDelim start=/__/ skip=/\\_/ end=/__/ oneline concealends

" delimiters: "(", ")", "'"
syntax match wikiDelimiter /[()"']/ containedin=ALLBUT,@wikiNoOverride display

" preformatted: "`<text>`" and "<pre>...</pre>"
syntax region wikiCode matchgroup=wikiCodeDelim start=/`\@<!`\(`\)\@!/ skip=/\\`/ end=/`\@<!`\(`\)\@!/ oneline concealends contains=@NoSpell
syntax region wikiPreBlock matchgroup=wikiHtmlBlock start=/<pre>/ end=/<\/pre>/ contains=@NoSpell

" LaTeX inline: "$ ... $"
syntax region wikiTexInline
    \ matchgroup=wikiTexDelim
    \ start=/\\\@<!\$\$\@!/
    \ skip=/\\\$/
    \ end=/\\\@<!\$/
    \ oneline keepend
    \ contains=@wikiTex,@texMathZoneGroup,@NoSpell

" LaTeX display: "$$ ... $$"
syntax region wikiTexDisplay
    \ matchgroup=wikiTexDelim
    \ start=/\\\@<!\$\$/
    \ end=/\\\@<!\$\$/
    \ keepend extend
    \ contains=wikiInnerColons,@wikiTex,@texMathZoneGroup,@NoSpell

" underline: "++<text>++"
syntax match wikiUnderlineFull /++.\{-}++/ display contains=wikiUnderlineDelim,wikiUnderlineValue
syntax match wikiUnderlineDelim /++/ contained display conceal
syntax match wikiUnderlineValue /\(++\)\@<=.\{-}\ze++/ contained display

" link: "[<label>](<url>)"
syntax match wikiLinkFull /\[.\{-}\](.\{-})/ display contains=wikiLinkText,wikiLinkUrl
syntax region wikiLinkText matchgroup=Comment start=/\[/ end=/\]/ contained oneline nextgroup=wikiLinkUrl
syntax region wikiLinkUrl matchgroup=Comment start=/(/ end=/)/ contained oneline

" title: "# <title>"
syntax match wikiTitleDelimiter /^\s*#\+/ display nextgroup=wikiTitleSpaces
syntax match wikiTitleSpaces /\s\+/ contained display nextgroup=wikiTitleText
syntax match wikiTitleText /.*$/ contained display

" lists: "<n>. <something>" and "- <something>"
syntax match wikiListMarker /^\s*\zs\(-\)\ze\s\+/ display
syntax match wikiOrderedListMarker /^\s*\zs\d\+[.)]\ze\s\+/ display

" line: ":: <entry>: <arg>=xxx, <arg>=yyy"
syntax match wikiRefsLine /^\s*::\s*__refs__:.*$/ display containedin=ALLBUT,@wikiNoOverride contains=wikiKeyword,@NoSpell
syntax match wikiColons /^\s*::\s*/ display containedin=ALLBUT,@wikiNoOverride contains=wikiRefsLine nextgroup=wikiKeyword
syntax match wikiInnerColons /^\s*::\s*/ contained display
syntax match wikiKeyword /[a-zA-Z0-9_-]\+:/ contained display
syntax match wikiArgName /\(^\s*::.*\)\@<=\<[a-zA-Z0-9_-]\+\ze=/ display containedin=ALLBUT,@wikiNoOverride nextgroup=wikiArgEquals
syntax match wikiArgEquals /=/ contained display nextgroup=wikiArgValue,wikiArgString
syntax match wikiArgValue /[^, "]\+/ contained display
syntax region wikiArgString start=/"/ skip=/\\"/ end=/"/ contained oneline display
syntax match wikiArgSeparator /\(^\s*::.*\)\@<=,/ display containedin=ALLBUT,@wikiNoOverride

" blocks: "{{ block.<name> }}"
syntax match wikiBlockBraces /{{\|}}/ contained display
syntax match wikiBlockWord /[a-zA-Z0-9_-]\+/ contained display
syntax match wikiBlockNamespace /[a-zA-Z0-9_-]\+\ze\./ contained display
syntax match wikiBlockDot /\./ contained display
syntax match wikiBlockName /\(\.\)\@<=[a-zA-Z0-9_-]\+/ contained display

" html: "<tag class='...'>...</tag>"
syntax region wikiCustomHtmlTag matchgroup=wikiHtmlBlock start=/<\/\?/ end=/>/ containedin=ALLBUT,@wikiNoOverride oneline display contains=wikiHtmlTagName,wikiHtmlArgName,wikiHtmlArgEquals,wikiHtmlArgString
syntax match wikiHtmlTagName /\(<\/\?\)\@<=[a-zA-Z0-9_-]\+/ contained display
syntax match wikiHtmlArgName /[a-zA-Z0-9_-]\+\ze=/ contained display
syntax match wikiHtmlArgEquals /=/ contained display
syntax region wikiHtmlArgString start=/"/ skip=/\\"/ end=/"/ contained oneline display

" refs: "{{ <topic>:<context>:<label> }}"
syntax match wikiRefDelimStart /{{\ze[^:]\+:[^:]\+:[^}]\+}}/ display containedin=ALLBUT,@wikiNoOverride conceal cchar=" nextgroup=wikiRefTopic
syntax match wikiRefTopic /[^:]\+/ contained display conceal nextgroup=wikiRefColon1
syntax match wikiRefColon1 /:/ contained display conceal nextgroup=wikiRefContext
syntax match wikiRefContext /[^:]\+/ contained display conceal nextgroup=wikiRefColon2
syntax match wikiRefColon2 /:/ contained display conceal nextgroup=wikiRefLabel
syntax match wikiRefLabel /[^}]\+/ contained display nextgroup=wikiRefDelimEnd
syntax match wikiRefDelimEnd /}}/ contained display conceal cchar="

" numbers
syntax match wikiNumber /\<\d\+\([.,]\d\+\)\?\>\([.)]\s\)\@!/ display containedin=ALLBUT,@wikiNoOverride

" block/tag arguments (e.g. lang=python)
syntax match wikiTagArgName /\<lang\ze=/ contained display nextgroup=wikiTagArgEquals
syntax match wikiTagArgEquals /=/ contained display nextgroup=wikiTagArgValue
syntax match wikiTagArgValue /[a-zA-Z0-9_-]\+/ contained display

" vars: "{{ <var> }}"
syntax region wikiVar matchgroup=wikiBlockBraces start=/{{\ze[^:]*}}/ end=/}}/ keepend oneline display containedin=ALLBUT,@wikiNoOverride contains=wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot,wikiTagArgName,wikiTagArgEquals,wikiTagArgValue

" Contained header rule so {{ block.code }}, {{ block.tex }}, {{ block.plain }} highlight identically to wikiVar
syntax region wikiBlockHeader matchgroup=wikiBlockBraces start=/{{\ze\s*block\.\(code\|tex\|plain\)\>/ end=/}}/ keepend oneline contained display contains=wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot,wikiTagArgName,wikiTagArgEquals,wikiTagArgValue,@NoSpell

" block.plain (hs=e+1 and me=s-1 exclude the opening/closing block lines from body highlight)
syntax region wikiPlainBlockBody
    \ start=/^\s*\%(::\s*\)\?{{\s*block\.plain\s*}}\s*\n/hs=e+1
    \ end=/^\s*\%(::\s*\)\?{{\s*block\.end\s*}}/me=s-1
    \ contains=wikiInnerColons,wikiBlockHeader,@NoSpell
    \ keepend extend

" block.code without lang=... (hs=e+1 and me=s-1 exclude the opening/closing block lines from body highlight)
syntax region wikiCodeBlockBody
    \ start=/^\s*\%(::\s*\)\?{{\s*block\.code\s*}}\s*\n/hs=e+1
    \ end=/^\s*\%(::\s*\)\?{{\s*block\.end\s*}}/me=s-1
    \ contains=wikiInnerColons,wikiBlockHeader,@NoSpell
    \ keepend extend

" block.tex (hs=e+1 and me=s-1 exclude the opening/closing block lines from body highlight)
syntax region wikiTexBlockBody
    \ start=/^\s*\%(::\s*\)\?{{\s*block\.tex\s*}}\s*\n/hs=e+1
    \ end=/^\s*\%(::\s*\)\?{{\s*block\.end\s*}}/me=s-1
    \ contains=wikiInnerColons,wikiBlockHeader,@wikiTex,@texMathZoneGroup,@NoSpell
    \ keepend extend

" block.code with lang=<ft>
for [s:marker, s:ft] in items(s:filetypes)
    if !has_key(s:loaded_fts, s:ft)
        continue
    endif

    execute 'syntax region wikiBlockBody_' . s:ft
        \ . ' start=/^\s*\%(::\s*\)\?{{\s*block\.code\s\+lang=' . s:marker . '\s*}}\s*\n/hs=e+1'
        \ . ' end=/^\s*\%(::\s*\)\?{{\s*block\.end\s*}}/me=s-1'
        \ . ' keepend extend contains=wikiInnerColons,wikiBlockHeader,@wikiCode_' . s:ft . ',@NoSpell'

    execute 'syntax cluster wikiNoOverride add=wikiBlockBody_' . s:ft . ',@wikiCode_' . s:ft
endfor

" HIGHLIGHT
highlight wikiItalic ctermfg=2 cterm=italic,underline
highlight default link wikiItalicDelim Comment
highlight wikiBold ctermfg=2 cterm=bold,underline
highlight default link wikiBoldDelim Comment
highlight default link wikiCodeDelim Comment
highlight wikiCode ctermfg=13 ctermbg=8
highlight! link wikiCodeBlockBody wikiCode
highlight default link wikiTexDelim Comment

highlight default link wikiDelimiter Delimiter
highlight default link wikiNumber Constant

highlight wikiLinkUrl ctermfg=4 cterm=underline
highlight wikiLinkText ctermfg=5
highlight wikiTitleDelimiter ctermfg=5
highlight wikiTitleText ctermfg=12 cterm=underline
highlight default link wikiListMarker Statement
highlight default link wikiOrderedListMarker Statement

highlight default link wikiColons Comment
highlight default link wikiInnerColons Comment
highlight default link wikiKeyword Delimiter
highlight default link wikiArgName Constant
highlight default link wikiArgEquals Operator
highlight default link wikiArgValue String
highlight default link wikiArgSeparator Operator
highlight default link wikiArgString String

highlight default link wikiBlockBraces Comment
highlight default link wikiBlockWord Title
highlight wikiBlockNamespace ctermfg=5
highlight default link wikiBlockDot Comment
highlight default link wikiBlockName Type

highlight! link wikiPlainBlockBody Normal

highlight default link wikiHtmlBlock Comment
highlight wikiHtmlTagName ctermfg=4
highlight default link wikiHtmlArgName Constant
highlight default link wikiHtmlArgEquals Operator
highlight default link wikiHtmlArgString String

highlight default link wikiUnderlineDelim Comment
highlight default link wikiUnderlineValue Underlined

highlight default link wikiRefDelimStart Comment
highlight default link wikiRefDelimEnd Comment
highlight default link wikiRefColon1 Comment
highlight default link wikiRefColon2 Comment
highlight default link wikiRefTopic Type
highlight default link wikiRefContext Identifier
highlight wikiRefLabel ctermfg=4 cterm=underline

highlight! link wikiTagArgName Constant
highlight! link wikiTagArgEquals Operator
highlight! link wikiTagArgValue String

setlocal conceallevel=2
syntax sync minlines=150 maxlines=500
let b:current_syntax = 'wiki'
