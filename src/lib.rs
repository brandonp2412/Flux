pub mod ast;
pub mod codegen;
pub mod parser;
pub mod typecheck;

pub fn compile_to_c(source: &str) -> Result<String, String> {
    let program = parser::parse(source)?;
    let signatures = typecheck::check(&program)?;
    codegen::emit_c(&program, &signatures)
}

pub fn check_source(source: &str) -> Result<(), String> {
    let program = parser::parse(source)?;
    typecheck::check(&program)?;
    Ok(())
}
