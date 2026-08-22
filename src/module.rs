//! The decoded shape of a WebAssembly module: value and function types, the
//! import/export tables, and the function bodies the interpreter runs. The
//! parser (not yet written) builds a [`Module`]; nothing in this file reads
//! bytes.
//!
//! Function indices span imports first, then locally defined functions, per
//! the specification - [`Module::func_type`] relies on that ordering instead
//! of tagging each index with where it came from, and the interpreter will
//! do the same once it exists.

use std::fmt;

/// A WebAssembly value type. Only the four MVP numeric types exist at the
/// binary format level; reference types are not part of what this crate
/// parses or runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
}

impl fmt::Display for ValType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ValType::I32 => "i32",
            ValType::I64 => "i64",
            ValType::F32 => "f32",
            ValType::F64 => "f64",
        })
    }
}

/// A function signature: zero or more parameters, zero or more results.
/// MVP-encoded modules never carry more than one result, but multi-value
/// blocks in later proposals allow more, so this stays a `Vec`.
///
/// ```
/// use wasm_fuel::module::{FuncType, ValType};
///
/// let square = FuncType { params: vec![ValType::I32], results: vec![ValType::I32] };
/// assert_eq!(square.to_string(), "(i32) -> i32");
///
/// let noop = FuncType { params: vec![], results: vec![] };
/// assert_eq!(noop.to_string(), "() -> ()");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

impl fmt::Display for FuncType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Params are always parenthesized, even when there is exactly one or
        // none, so a signature reads left-to-right as "inputs -> output".
        // Results collapse to a bare type when there is exactly one, since
        // that is the overwhelmingly common MVP case and "(i32) -> (i32)"
        // is just noise.
        write_parenthesized(f, &self.params)?;
        f.write_str(" -> ")?;
        match self.results.as_slice() {
            [single] => write!(f, "{single}"),
            many => write_parenthesized(f, many),
        }
    }
}

fn write_parenthesized(f: &mut fmt::Formatter<'_>, types: &[ValType]) -> fmt::Result {
    f.write_str("(")?;
    for (i, t) in types.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{t}")?;
    }
    f.write_str(")")
}

/// The kind of thing an import or export refers to, matching the single byte
/// the binary format uses to tag both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternKind {
    Func,
    Table,
    Memory,
    Global,
}

impl ExternKind {
    /// Decodes the kind byte used in the import and export sections.
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(ExternKind::Func),
            0x01 => Some(ExternKind::Table),
            0x02 => Some(ExternKind::Memory),
            0x03 => Some(ExternKind::Global),
            _ => None,
        }
    }
}

/// What an import provides. Only `Func` carries a payload the interpreter
/// cares about (its type index); table, memory and global imports still
/// consume a function-index-like slot in their own index spaces, but nothing
/// in this crate calls into them, so their descriptors are not decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportDesc {
    Func(u32),
    Table,
    Memory,
    Global,
}

/// A single entry of the import section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub desc: ImportDesc,
}

/// A single entry of the export section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub name: String,
    pub kind: ExternKind,
    pub index: u32,
}

/// A locally defined function: its signature (by index into
/// [`Module::types`]), its declared locals beyond the parameters, and its
/// body as raw instruction bytes including the trailing `end` opcode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Func {
    pub type_idx: u32,
    pub locals: Vec<ValType>,
    pub body: Vec<u8>,
}

/// A fully decoded module.
///
/// ```
/// use wasm_fuel::module::{Export, ExternKind, Func, FuncType, Module, ValType};
///
/// let module = Module {
///     types: vec![FuncType { params: vec![ValType::I32], results: vec![ValType::I32] }],
///     imports: vec![],
///     funcs: vec![Func { type_idx: 0, locals: vec![], body: vec![0x0B] }],
///     exports: vec![Export { name: "square".into(), kind: ExternKind::Func, index: 0 }],
///     start: None,
///     custom_sections: vec![],
///     skipped_sections: vec![],
/// };
///
/// assert_eq!(module.export_func("square"), Some(0));
/// assert_eq!(module.describe_exports(), vec!["func square: (i32) -> i32"]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub types: Vec<FuncType>,
    pub imports: Vec<Import>,
    pub funcs: Vec<Func>,
    pub exports: Vec<Export>,
    pub start: Option<u32>,
    pub custom_sections: Vec<String>,
    pub skipped_sections: Vec<u8>,
}

impl Module {
    /// How many function imports precede the locally defined functions in
    /// the function index space.
    pub fn imported_func_count(&self) -> u32 {
        self.imports
            .iter()
            .filter(|i| matches!(i.desc, ImportDesc::Func(_)))
            .count() as u32
    }

    /// The signature of the function at `index` in the function index space
    /// (imports first, then locals), or `None` if the index is out of range.
    pub fn func_type(&self, index: u32) -> Option<&FuncType> {
        let imported = self.imported_func_count();
        let type_idx = if index < imported {
            self.imports
                .iter()
                .filter_map(|i| match i.desc {
                    ImportDesc::Func(t) => Some(t),
                    _ => None,
                })
                .nth(index as usize)?
        } else {
            self.funcs.get((index - imported) as usize)?.type_idx
        };
        self.types.get(type_idx as usize)
    }

    /// The function index exported under `name`, if there is one.
    pub fn export_func(&self, name: &str) -> Option<u32> {
        self.exports
            .iter()
            .find(|e| e.kind == ExternKind::Func && e.name == name)
            .map(|e| e.index)
    }

    /// One line per function export, `"func <name>: <signature>"`, in export
    /// order. Non-function exports are omitted.
    pub fn describe_exports(&self) -> Vec<String> {
        self.exports
            .iter()
            .filter(|e| e.kind == ExternKind::Func)
            .filter_map(|e| {
                let ty = self.func_type(e.index)?;
                Some(format!("func {}: {}", e.name, ty))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn func_type(params: &[ValType], results: &[ValType]) -> FuncType {
        FuncType { params: params.to_vec(), results: results.to_vec() }
    }

    #[test]
    fn formats_signatures() {
        assert_eq!(func_type(&[], &[]).to_string(), "() -> ()");
        assert_eq!(func_type(&[ValType::I32], &[ValType::I64]).to_string(), "(i32) -> i64");
        assert_eq!(
            func_type(&[ValType::I32, ValType::F64], &[ValType::I32]).to_string(),
            "(i32, f64) -> i32"
        );
        assert_eq!(func_type(&[], &[ValType::I32, ValType::I32]).to_string(), "() -> (i32, i32)");
    }

    #[test]
    fn decodes_extern_kind_bytes() {
        assert_eq!(ExternKind::from_byte(0x00), Some(ExternKind::Func));
        assert_eq!(ExternKind::from_byte(0x01), Some(ExternKind::Table));
        assert_eq!(ExternKind::from_byte(0x02), Some(ExternKind::Memory));
        assert_eq!(ExternKind::from_byte(0x03), Some(ExternKind::Global));
        assert_eq!(ExternKind::from_byte(0x04), None);
    }

    fn sample_module() -> Module {
        Module {
            types: vec![
                func_type(&[ValType::I32], &[ValType::I32]), // 0: (i32) -> i32
                func_type(&[], &[]),                          // 1: () -> ()
            ],
            imports: vec![
                Import { module: "env".into(), name: "log".into(), desc: ImportDesc::Func(1) },
                Import { module: "env".into(), name: "mem".into(), desc: ImportDesc::Memory },
            ],
            funcs: vec![Func { type_idx: 0, locals: vec![], body: vec![0x0B] }],
            exports: vec![
                Export { name: "square".into(), kind: ExternKind::Func, index: 1 }, // local func
                Export { name: "log".into(), kind: ExternKind::Func, index: 0 },    // imported func
                Export { name: "mem".into(), kind: ExternKind::Memory, index: 0 },
            ],
            start: None,
            custom_sections: vec![],
            skipped_sections: vec![],
        }
    }

    #[test]
    fn counts_only_function_imports() {
        assert_eq!(sample_module().imported_func_count(), 1);
    }

    #[test]
    fn func_type_spans_imports_then_locals() {
        let module = sample_module();
        // Index 0 is the imported "log", type 1: () -> ().
        assert_eq!(module.func_type(0), Some(&func_type(&[], &[])));
        // Index 1 is the local function, type 0: (i32) -> i32.
        assert_eq!(module.func_type(1), Some(&func_type(&[ValType::I32], &[ValType::I32])));
        assert_eq!(module.func_type(2), None);
    }

    #[test]
    fn export_func_ignores_non_function_exports() {
        let module = sample_module();
        assert_eq!(module.export_func("square"), Some(1));
        assert_eq!(module.export_func("mem"), None);
        assert_eq!(module.export_func("missing"), None);
    }

    #[test]
    fn describe_exports_lists_functions_in_export_order() {
        let module = sample_module();
        assert_eq!(
            module.describe_exports(),
            vec!["func square: (i32) -> i32".to_string(), "func log: () -> ()".to_string()]
        );
    }
}
