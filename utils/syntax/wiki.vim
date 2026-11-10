let s:current_dir = expand('<sfile>:p:h')

if hlexists('wikiCode')
    syntax clear wikiCode
endif
if hlexists('wikiCodeBlock')
   syntax clear wikiCodeBlock
endif

" italic: "_<text>_"
syntax region wikiItalic matchgroup=wikiItalicDelim start=/_/ skip=/\\_/ end=/_/ concealends

" bold: "__<text>__"
syntax region wikiBold matchgroup=wikiBoldDelim start=/__/ skip=/\\_/ end=/__/ concealends

" delimiters: "(", ")", "'"
syntax match wikiDelimiter /[()"']/ containedin=ALL

" preformatted: "`<text>`" and "<pre>...</pre>"
syntax region wikiCode matchgroup=wikiCodeDelim start=/`\@<!`\(`\)\@!/ skip=/\\`/ end=/`\@<!`\(`\)\@!/ concealends
syntax region wikiPreBlock matchgroup=wikiHtmlBlock start=/<pre>/ end=/<\/pre>/

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
syntax match wikiRefsLine /^\s*::\s*__refs__:.*$/ containedin=ALL contains=wikiKeyword,@NoSpell
syntax match wikiColons /^\s*::\s*/ containedin=ALL contains=wikiRefsLine nextgroup=wikiKeyword
syntax match wikiKeyword /[a-zA-Z0-9_-]\+:/ contained
syntax match wikiArgName /\(^\s*::.*\)\@<=\<[a-zA-Z0-9_-]\+\ze=/ containedin=ALL nextgroup=wikiArgEquals
syntax match wikiArgEquals /=/ contained nextgroup=wikiArgValue,wikiArgString
syntax match wikiArgValue /[^, "]\+/ contained
syntax region wikiArgString start=/"/ skip=/\\"/ end=/"/ contained
syntax match wikiArgSeparator /\(^\s*::.*\)\@<=,/ containedin=ALL

" blocks: "{{ block.<name> }}"
syntax match wikiBlockBraces /{{\|}}/ contained
syntax match wikiBlockWord /[a-zA-Z0-9_-]\+/ contained
syntax match wikiBlockNamespace /[a-zA-Z0-9_-]\+\ze\./ contained
syntax match wikiBlockDot /\./ contained
syntax match wikiBlockName /\(\.\)\@<=[a-zA-Z0-9_-]\+/ contained

" html: "<tag class='...'>...</tag>"
syntax region wikiCustomHtmlTag matchgroup=wikiHtmlBlock start=/<\/\?/ end=/>/ containedin=ALLBUT,wikiCode oneline contains=wikiHtmlTagName,wikiHtmlArgName,wikiHtmlArgEquals,wikiHtmlArgString
syntax match wikiHtmlTagName /\(<\/\?\)\@<=[a-zA-Z0-9_-]\+/ contained
syntax match wikiHtmlArgName /[a-zA-Z0-9_-]\+\ze=/ contained
syntax match wikiHtmlArgEquals /=/ contained
syntax region wikiHtmlArgString start=/"/ skip=/\\"/ end=/"/ contained

" refs: "{{ <topic>:<context>:<label> }}"
syntax match wikiRefDelimStart /{{\ze[^:]\+:[^:]\+:[^}]\+}}/ containedin=ALL conceal cchar=" nextgroup=wikiRefTopic
syntax match wikiRefTopic /[^:]\+/ contained conceal nextgroup=wikiRefColon1
syntax match wikiRefColon1 /:/ contained conceal nextgroup=wikiRefContext
syntax match wikiRefContext /[^:]\+/ contained conceal nextgroup=wikiRefColon2
syntax match wikiRefColon2 /:/ contained conceal nextgroup=wikiRefLabel
syntax match wikiRefLabel /[^}]\+/ contained nextgroup=wikiRefDelimEnd
syntax match wikiRefDelimEnd /}}/ contained conceal cchar="

" numbers
syntax match wikiNumber /\<\d\+\([.,]\d\+\)\?\>\([.)]\s\)\@!/ containedin=ALL

" vars: "{{ <var> }}"
silent! syntax clear wikiVar
syntax region wikiVar matchgroup=wikiBlockBraces start=/{{\ze[^:]*}}/ end=/}}/ keepend oneline containedin=ALL contains=wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot

" block/tag arguments (e.g. lang=python)
syntax match wikiTagArgName /\<lang\ze=/ contained nextgroup=wikiTagArgEquals
syntax match wikiTagArgEquals /=/ contained nextgroup=wikiTagArgValue
syntax match wikiTagArgValue /[a-zA-Z0-9_-]\+/ contained

" block.plain
silent! syntax clear wikiPlainBlockStart
silent! syntax clear wikiPlainBlockBody
syntax match wikiPlainBlockStart /^\s*{{\s*block\.plain\s*}}/ contains=wikiBlockBraces,wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot nextgroup=wikiPlainBlockBody skipnl
syntax region wikiPlainBlockBody start=/^/ end=/^\s*\ze{{\s*block\.end\s*}}/ contained

" block.end 
syntax match wikiBlockEnd /^\s*{{\s*block\.end\s*}}/ contains=wikiBlockBraces,wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot

" block.code
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
    execute 'silent! syntax include @wikiCode_' . s:ft . ' syntax/' . s:ft . '.vim'
    
    execute 'syntax match wikiBlockCodeStart_' . s:ft
        \ . ' /^\s*{{\s*block\.code\s\+lang=' . s:marker . '\s*}}/'
        \ . ' contains=wikiBlockBraces,wikiBlockWord,wikiBlockNamespace,wikiBlockName,wikiBlockDot,wikiTagArgName,wikiTagArgEquals,wikiTagArgValue'
        \ . ' nextgroup=wikiBlockBody_' . s:ft . ' skipnl'
        
    execute 'syntax region wikiBlockBody_' . s:ft
        \ . ' start=/^/'
        \ . ' end=/^\s*\ze{{\s*block\.end\s*}}/'
        \ . ' contained contains=@wikiCode_' . s:ft
endfor


" HIGHLIGHT
hi WikiItalic ctermfg=2 cterm=italic,underline
hi link WikiItalicDelim Comment
hi WikiBold ctermfg=2 cterm=bold,underline
hi link WikiBoldDelim Comment
hi link wikiCodeDelim Comment
hi wikiCode ctermfg=13 cterm=underline
hi link wikiCodeBlock Code

hi link WikiDelimiter Delimiter
hi link WikiNumber Constant

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
hi link wikiBlockWord Title 
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
