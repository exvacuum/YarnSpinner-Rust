//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/da39c7195107d8211f21c263e4084f773b84eaff/YarnSpinner.Compiler/Compiler.cs>

pub(crate) use self::{antlr_rust_ext::*, utils::*};
use crate::listeners::*;
use crate::output::*;
use crate::prelude::generated::yarnspinnerparser::YarnSpinnerParserTreeWalker;
use crate::prelude::FileParseResult;
use crate::string_table_manager::StringTableManager;
use crate::visitors::*;
use antlr_rust::tree::ParseTreeVisitorCompat;
pub use compilation_job::*;
use std::collections::{HashMap, HashSet};
use yarn_slinger_core::prelude::{Library, Operand};
use yarn_slinger_core::types::*;

mod antlr_rust_ext;
mod compilation_job;
mod utils;

/// Compile Yarn code, as specified by a compilation job.
pub fn compile(compilation_job: CompilationJob) -> CompilationResult {
    // TODO: other steps
    let compiler_steps: Vec<&CompilationStep> = vec![
        &register_strings,
        &get_declarations,
        &check_types,
        &find_tracking_nodes,
        &add_tracking_declarations,
        &generate_code,
        &add_initial_value_registrations,
    ];

    let initial = CompilationIntermediate::from_job(&compilation_job);
    compiler_steps
        .into_iter()
        .fold(initial, |state, step| step(state))
        .result
        .unwrap()
}

type CompilationStep = dyn Fn(CompilationIntermediate) -> CompilationIntermediate;

fn get_declarations(mut state: CompilationIntermediate) -> CompilationIntermediate {
    // Find the variable declarations in these files.
    for file in &state.parsed_files {
        let mut variable_declaration_visitor =
            DeclarationVisitor::new(state.known_variable_declarations.clone(), file.clone());

        variable_declaration_visitor.visit(file.tree.as_ref());

        state
            .known_variable_declarations
            .extend(variable_declaration_visitor.new_declarations.clone());
        state
            .derived_variable_declarations
            .extend(variable_declaration_visitor.new_declarations);

        state
            .diagnostics
            .extend_from_slice(&variable_declaration_visitor.diagnostics);
        state
            .file_tags
            .insert(file.name.clone(), variable_declaration_visitor.file_tags);
    }
    state
}

fn register_strings(mut state: CompilationIntermediate) -> CompilationIntermediate {
    // First pass: parse all files, generate their syntax trees,
    // and figure out what variables they've declared
    for file in &state.job.files {
        let parse_result = parse_syntax_tree(file, &mut state.diagnostics);

        // ok now we will add in our lastline tags
        // we do this BEFORE we build our strings table otherwise the tags will get missed
        // this should probably be a flag instead of every time though
        let mut last_line_tagger = LastLineBeforeOptionsVisitor::default();
        last_line_tagger.visit(parse_result.tree.as_ref());

        let mut visitor =
            StringTableGeneratorVisitor::new(state.string_table.clone(), parse_result.clone());
        visitor.visit(parse_result.tree.as_ref());
        state.diagnostics.extend(visitor.diagnostics);
        state.string_table.extend(visitor.string_table_manager);
        state.parsed_files.push(parse_result);
    }

    state
}

fn find_tracking_nodes(mut state: CompilationIntermediate) -> CompilationIntermediate {
    // determining the nodes we need to track visits on
    // this needs to be done before we finish up with declarations
    // so that any tracking variables are included in the compiled declarations
    let mut tracking_nodes = HashSet::new();
    let mut ignore_nodes = HashSet::new();
    for file in &state.parsed_files {
        let mut visitor = NodeTrackingVisitor::new();
        visitor.visit(file.tree.as_ref());
        tracking_nodes.extend(visitor.tracking_nodes);
        ignore_nodes.extend(visitor.ignoring_nodes);
    }
    state.tracking_nodes = tracking_nodes.difference(&ignore_nodes).cloned().collect();
    state
}

fn check_types(mut state: CompilationIntermediate) -> CompilationIntermediate {
    for file in &state.parsed_files {
        let mut visitor =
            TypeCheckVisitor::new(state.known_variable_declarations.clone(), file.clone());
        visitor.visit(file.tree.as_ref());
        state
            .known_variable_declarations
            .extend(visitor.new_declarations.clone());
        state
            .derived_variable_declarations
            .extend(visitor.new_declarations);
        state.diagnostics.extend(visitor.diagnostics);
        state.potential_issues.extend(visitor.deferred_types);
        state.known_types.extend(visitor.known_types);
    }
    state
}

fn add_tracking_declarations(mut state: CompilationIntermediate) -> CompilationIntermediate {
    let tracking_declarations: Vec<_> = state
        .tracking_nodes
        .iter()
        .map(|node| {
            Declaration::default()
                .with_default_value(0.)
                .with_name(Library::generate_unique_visited_variable_for_node(node))
                .with_type(Type::Number)
                .with_description(format!(
                    "The generated variable for tracking visits of node {node}"
                ))
        })
        .collect();

    // adding the generated tracking variables into the declaration list
    // this way any future variable storage system will know about them
    // if we didn't do this later stages wouldn't be able to interface with them
    state
        .known_variable_declarations
        .extend(tracking_declarations.clone());
    state
        .derived_variable_declarations
        .extend(tracking_declarations);
    state
}

fn generate_code(mut state: CompilationIntermediate) -> CompilationIntermediate {
    let has_errors = state
        .diagnostics
        .iter()
        .any(|d| d.severity == DiagnosticSeverity::Error);
    let results: Vec<_> = if has_errors {
        // We have errors, so we can't safely generate code.
        vec![]
    } else {
        // No errors! Go ahead and generate the code for all parsed files.
        let template = CompilationResult {
            string_table: state.string_table.0.clone(),
            contains_implicit_string_tags: state.string_table.contains_implicit_string_tags(),
            ..Default::default()
        };
        state
            .parsed_files
            .iter()
            .map(|file| {
                generate_code_for_file(
                    &mut state.tracking_nodes,
                    state.known_types.clone(),
                    template.clone(),
                    file,
                )
            })
            .collect()
    };
    state.result = Some(CompilationResult::combine(
        results,
        state.string_table.clone(),
    ));
    state
}

fn generate_code_for_file<'a, 'b: 'a, 'input: 'a + 'b>(
    tracking_nodes: &mut HashSet<String>,
    known_types: KnownTypes,
    result_template: CompilationResult,
    file: &'a FileParseResult<'input>,
) -> CompilationResult {
    let compiler_listener = Box::new(CompilerListener::new(
        tracking_nodes.clone(),
        known_types,
        file.clone(),
    ));
    let compiler_tracking_nodes = compiler_listener.tracking_nodes.clone();
    let compiler_diagnostics = compiler_listener.diagnostics.clone();
    let compiler_program = compiler_listener.program.clone();
    let compiler_debug_infos = compiler_listener.debug_infos.clone();

    YarnSpinnerParserTreeWalker::walk(compiler_listener, file.tree.as_ref());

    tracking_nodes.extend(compiler_tracking_nodes.borrow().iter().cloned());

    // Don't attempt to generate debug information if compilation produced errors
    if compiler_diagnostics
        .borrow()
        .iter()
        .any(|d| d.severity == DiagnosticSeverity::Error)
    {
        CompilationResult {
            // ## Implementation notes
            // In the original, this could still contain a `Program` even though the docs say otherwise
            program: None,
            diagnostics: compiler_diagnostics.borrow().clone(),
            ..result_template
        }
    } else {
        let debug_infos: HashMap<_, _> = compiler_debug_infos
            .borrow()
            .iter()
            .map(|debug_info| (debug_info.node_name.clone(), debug_info.clone()))
            .collect();

        CompilationResult {
            program: Some(compiler_program.borrow().clone()),
            diagnostics: compiler_diagnostics.borrow().clone(),
            debug_info: debug_infos,
            ..result_template
        }
    }
}

fn add_initial_value_registrations(mut state: CompilationIntermediate) -> CompilationIntermediate {
    // Last step: take every variable declaration we found in all
    // of the inputs, and create an initial value registration for
    // it.
    let declarations = state
        .known_variable_declarations
        .iter()
        .filter(|decl| !matches!(decl.r#type, Some(Type::Function(_))))
        .filter(|decl| decl.r#type.is_some());
    let result = state.result.as_mut().unwrap();

    for declaration in declarations {
        let Some(default_value) = declaration.default_value.clone() else {
             result.diagnostics.push(
                 Diagnostic::from_message(
                     format!("Variable declaration {} (type {}) has a null default value. This is not allowed.", declaration.name, declaration.r#type.format())));
             continue;
         };
        if let Some(ref mut program) = result.program {
            let value = match declaration.r#type.as_ref().unwrap() {
                Type::String => Operand::from(String::try_from(default_value).unwrap()),
                Type::Number => Operand::from(f32::try_from(default_value).unwrap()),
                Type::Boolean => Operand::from(bool::try_from(default_value).unwrap()),
                _ => panic!("Cannot create initial value registration for type {}. This is a bug. Please report it at https://github.com/Mafii/yarn_slinger/issues/new", declaration.r#type.format()),
            };
            program
                .initial_values
                .insert(declaration.name.clone(), value);
        }
    }
    result.declarations = state.derived_variable_declarations.clone();
    let unique_diagnostics: HashSet<Diagnostic> =
        HashSet::from_iter(state.diagnostics.clone().into_iter());
    result.diagnostics = unique_diagnostics.into_iter().collect();
    state
}

struct CompilationIntermediate<'input> {
    job: &'input CompilationJob,
    result: Option<CompilationResult>,
    known_variable_declarations: Vec<Declaration>,
    derived_variable_declarations: Vec<Declaration>,
    potential_issues: Vec<DeferredTypeDiagnostic>,
    parsed_files: Vec<FileParseResult<'input>>,
    tracking_nodes: HashSet<String>,
    string_table: StringTableManager,
    diagnostics: Vec<Diagnostic>,
    file_tags: HashMap<String, Vec<String>>,
    known_types: KnownTypes,
}

impl<'input> CompilationIntermediate<'input> {
    pub(crate) fn from_job(compilation_job: &'input CompilationJob) -> Self {
        Self {
            job: compilation_job,
            result: Default::default(),
            known_variable_declarations: Default::default(),
            derived_variable_declarations: Default::default(),
            potential_issues: Default::default(),
            parsed_files: Default::default(),
            tracking_nodes: Default::default(),
            string_table: Default::default(),
            diagnostics: Default::default(),
            file_tags: Default::default(),
            known_types: Default::default(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn can_call_compile_empty_without_crash() {
        compile(CompilationJob {
            files: vec![],
            library: None,
            compilation_type: CompilationType::FullCompilation,
            variable_declarations: vec![],
        });
    }

    #[test]
    fn can_call_compile_file_without_crash() {
        let file = File {
            file_name: "test.yarn".to_string(),
            source: "title: test
---
foo
bar
a {1 + 3} cool expression
==="
            .to_string(),
        };
        compile(CompilationJob {
            files: vec![file],
            library: None,
            compilation_type: CompilationType::FullCompilation,
            variable_declarations: vec![],
        });
    }
}
