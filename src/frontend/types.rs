//! Define the types in AST of SysY language.
//! The types are used in the AST and the symbol table.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::{fmt, hash};

/// The type in AST
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeKind {
    Void,
    /// The boolean type.
    ///
    /// There is no `bool` type, but the intermediate result might be boolean.
    Bool,
    /// The integer type.
    Int,
    /// The function type, with params and return type.
    Func(Vec<Type>, Type),
}

// The type in AST
#[derive(Clone, Eq)]
pub struct Type(Rc<TypeKind>);

impl hash::Hash for Type {
    fn hash<H: hash::Hasher>(&self, state: &mut H) { self.0.hash(state) }
}

impl PartialEq for Type {
    // Just compare the pointers
    fn eq(&self, other: &Self) -> bool { Rc::ptr_eq(&self.0, &other.0) }
}

impl fmt::Debug for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
}

impl fmt::Display for Type {
    /// Display the type.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            TypeKind::Void => write!(f, "void"),
            TypeKind::Bool => write!(f, "bool"),
            TypeKind::Int => write!(f, "int"),
            TypeKind::Func(params, ret) => write!(
                f,
                "{}({})",
                ret,
                params
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl Type {
    thread_local! {
        /// The pool to implement singleton.
        ///
        /// Reference: https://github.com/pku-minic/koopa/blob/master/src/ir/types.rs
        ///
        /// XXX: This is not the only solution. In the implementation of IR, we use
        /// `UniqueArena` to store types.
        static POOL: RefCell<HashMap<TypeKind, Type>> = RefCell::new(HashMap::default());
    }

    /// Create a new type.
    pub fn make(kind: TypeKind) -> Type {
        Self::POOL.with(|pool| {
            let mut pool = pool.borrow_mut();
            if let Some(ty) = pool.get(&kind) {
                ty.clone()
            } else {
                let ty = Type(Rc::new(kind.clone()));
                pool.insert(kind, ty.clone());
                ty
            }
        })
    }

    /// Get the kind of the type.
    pub fn kind(&self) -> &TypeKind { &self.0 }

    /// Create a new void type.
    pub fn void() -> Self { Self::make(TypeKind::Void) }

    /// Create a new boolean type.
    pub fn bool() -> Self { Self::make(TypeKind::Bool) }

    /// Create a new integer type.
    pub fn int() -> Self { Self::make(TypeKind::Int) }

    /// Create a new function type.
    pub fn func(params: Vec<Type>, ret: Type) -> Self { Self::make(TypeKind::Func(params, ret)) }

    /// Check if the type is a int type.
    pub fn is_int(&self) -> bool { matches!(self.kind(), TypeKind::Int) }

    /// Check if the type is a bool type.
    pub fn is_bool(&self) -> bool { matches!(self.kind(), TypeKind::Bool) }

    /// Check if the type is a void type.
    pub fn is_void(&self) -> bool { matches!(self.kind(), TypeKind::Void) }

    /// Get the parameters and return type of a function type.
    ///
    /// # Panics
    ///
    /// - Panics if the type is not a function type.
    pub fn unwrap_func(&self) -> (&[Type], &Type) {
        if let TypeKind::Func(params, ret) = self.kind() {
            (params, ret)
        } else {
            panic!("unwrap_func: not a function type: {}", self);
        }
    }

    /// Get the bytewidth of the type.
    pub fn bytewidth(&self) -> usize {
        match self.kind() {
            TypeKind::Void => 0,
            TypeKind::Bool => 1,
            TypeKind::Int => 4,
            TypeKind::Func(_, _) => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_display() {
        assert_eq!(Type::void().to_string(), "void");
        assert_eq!(Type::bool().to_string(), "bool");
        assert_eq!(Type::int().to_string(), "int");
    }

    #[test]
    fn test_type_creation() {
        let void_type = Type::void();
        let bool_type = Type::bool();
        let int_type = Type::int();

        assert!(void_type.is_void());
        assert!(bool_type.is_bool());
        assert!(int_type.is_int());
    }

    #[test]
    fn test_function_type() {
        let return_type = Type::int();
        let param_types = vec![Type::bool(), Type::int()];
        let func_type = Type::func(param_types.clone(), return_type.clone());

        let (params, ret) = func_type.unwrap_func();
        assert_eq!(params.to_vec(), param_types);
        assert_eq!(*ret, return_type);
    }

    #[test]
    #[should_panic(expected = "unwrap_func: not a function type")]
    fn test_unwrap_func_panic() {
        let int_type = Type::int();
        int_type.unwrap_func();
    }

    #[test]
    fn test_bytewidth() {
        assert_eq!(Type::void().bytewidth(), 0);
        assert_eq!(Type::bool().bytewidth(), 1);
        assert_eq!(Type::int().bytewidth(), 4);
    }

    #[test]
    fn test_singleton_type_creation() {
        let int_type1 = Type::int();
        let int_type2 = Type::int();

        assert!(Rc::ptr_eq(&int_type1.0, &int_type2.0));
    }
}
