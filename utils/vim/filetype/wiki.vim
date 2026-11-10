function! s:TextFormatters() abort
    inoremap <buffer> <C-l> <C-r>=__\<Left><CR>
    inoremap <buffer> <C-b> <C-r>=____\<Left>\<Left><CR>
    inoremap <buffer> <C-u> <C-r>=++\<Left>\<Left>\<Left>\<Left><CR>
    xnoremap <buffer> <C-i> c_<C-r>"_<Esc>i
    xnoremap <buffer> <C-l> c_<C-r>"_<Esc>i
    xnoremap <buffer> <C-b> c__<C-r>"__<Esc>i
    xnoremap <buffer> <C-u> c+<C-r>"+<Esc>i
endfunction

let s:wiki_words = {}
let s:last_char_time = reltime()
let s:complete_timer = -1

function! s:RefreshWikiWords() abort
    let s:wiki_words = {}
    for l:line in getline(1, '$')
        for l:w in split(l:line, '\W\+')
            if len(l:w) >= 3
                let s:wiki_words[l:w] = 1
            endif
        endfor
    endfor
endfunction

function! s:CompleteWikiWords() abort
    let s:complete_timer = -1
    if mode() !=# 'i' || &paste || pumvisible()
        return
    endif
    let l:col = col('.') - 1
    let l:line = strpart(getline('.'), 0, l:col)
    if l:line =~# '\<def\.[^[:space:]]*$'
        return
    endif
    let l:start = match(l:line, '\k\{2,}$')
    if l:start < 0
        return
    endif
    let l:prefix = strpart(l:line, l:start)
    let l:matches = []
    for l:w in keys(s:wiki_words)
        if stridx(l:w, l:prefix) == 0 && l:w !=# l:prefix
            call add(l:matches, l:w)
        endif
    endfor
    if !empty(l:matches)
        call complete(l:start + 1, sort(l:matches))
    endif
endfunction

function! s:OnInsertCharPre() abort
    if s:complete_timer != -1
        call timer_stop(s:complete_timer)
        let s:complete_timer = -1
    endif
    let l:now = reltime()
    let l:elapsed_ms = reltimefloat(reltime(s:last_char_time, l:now)) * 1000.0
    let s:last_char_time = l:now
    if l:elapsed_ms < 25.0
        if pumvisible()
            call feedkeys("\<C-e>", 'n')
        endif
        return
    endif
    let s:complete_timer = timer_start(80, {-> s:CompleteWikiWords()})
endfunction

setlocal completeopt=menuone,noinsert,noselect
setlocal complete-=k

call s:TextFormatters()
call s:RefreshWikiWords()

augroup WikiAutoComplete
    autocmd! * <buffer>
    autocmd InsertLeave <buffer> call s:RefreshWikiWords()
    autocmd InsertCharPre <buffer> call s:OnInsertCharPre()
augroup END
