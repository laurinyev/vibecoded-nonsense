use crate::ast::Type;
#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub ty: Type,
    pub is_register: bool,
}
#[derive(Clone, Debug)]
pub struct SymbolError(pub String);
pub trait SymbolTable {
    fn enter_scope(&mut self);
    fn exit_scope(&mut self);
    fn insert(&mut self, symbol: Symbol) -> Result<(), SymbolError>;
    fn lookup(&self, name: &str) -> Option<&Symbol>;
    fn lookup_current_scope(&self, name: &str) -> Option<&Symbol>;
}
pub struct Scopes {
    pub scopes: Vec<std::collections::HashMap<String, Symbol>>,
}
impl Default for Scopes {
    fn default() -> Self {
        Self::new()
    }
}
impl Scopes {
    pub fn new() -> Self {
        Self {
            scopes: vec![Default::default()],
        }
    }
}
impl SymbolTable for Scopes {
    fn enter_scope(&mut self) {
        self.scopes.push(Default::default())
    }
    fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }
    fn insert(&mut self, s: Symbol) -> Result<(), SymbolError> {
        let x = self.scopes.last_mut().unwrap();
        if x.contains_key(&s.name) {
            Err(SymbolError(format!("duplicate symbol {}", s.name)))
        } else {
            x.insert(s.name.clone(), s);
            Ok(())
        }
    }
    fn lookup(&self, n: &str) -> Option<&Symbol> {
        self.scopes.iter().rev().find_map(|x| x.get(n))
    }
    fn lookup_current_scope(&self, n: &str) -> Option<&Symbol> {
        self.scopes.last().unwrap().get(n)
    }
}
