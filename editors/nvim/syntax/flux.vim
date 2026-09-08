if exists("b:current_syntax")
  finish
endif

syntax keyword fluxKeyword fn return if elif else for while break continue match interface impl import pub type view app state grid at span
syntax keyword fluxDeclaration let var const struct enum
syntax keyword fluxBoolean true false nil
syntax keyword fluxType i64 bool str error void
syntax keyword fluxBuiltin print error
syntax match fluxNumber "\<\d\+\>"
syntax region fluxString start=+"+ skip=+\\"+ end=+"+
syntax match fluxComment "#.*$"
syntax match fluxOperator "->\|=>\|\.\.\|==\|!=\|<=\|>=\|[+*/%=<>!-]"

highlight default link fluxKeyword Keyword
highlight default link fluxDeclaration Statement
highlight default link fluxBoolean Boolean
highlight default link fluxType Type
highlight default link fluxBuiltin Function
highlight default link fluxNumber Number
highlight default link fluxString String
highlight default link fluxComment Comment
highlight default link fluxOperator Operator

let b:current_syntax = "flux"
