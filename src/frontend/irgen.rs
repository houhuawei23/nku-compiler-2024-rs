//! IR generation from AST.

use super::ast::{
    self,
    BinaryOp,
    BlockItem,
    CompUnit,
    ComptimeVal as Cv, // alias, represents conptime values in the AST
    ConstDecl,
    ConstDef,
    Decl,
    Expr,
    ExprKind,
    ExprStmt,
    FuncDef,
    FuncFParam,
    Item,
    ReturnStmt,
    Stmt,
    SymbolEntry,
    SymbolTable,
    VarDecl,
    VarDef,
};
use super::types::{Type, TypeKind as Tk};
use crate::frontend::ast::{FuncCall, LVal, UnaryOp};
use crate::infra::linked_list::LinkedListContainer;
use crate::ir::{self, Block, ConstantValue, Context, Func, Global, Inst, TargetInfo, Ty, Value};

/// Generate IR from the AST.
pub fn irgen(ast: &CompUnit, pointer_width: u8) -> Context {
    let mut irgen = IrGenContext::default();

    // Set pointer width for target platform
    irgen.ctx.set_target_info(TargetInfo {
        ptr_size: pointer_width as u32,
    });

    // Generate IR
    ast.irgen(&mut irgen);

    // Transfer ownership of the generated IR.
    irgen.finish()
}

/// Generated IR result.
/// Its used to map AST nodes to IR values.
/// It can be either a Global or a Value.
#[derive(Debug, Clone, Copy)]
pub enum IrGenResult {
    Global(Global),
    Value(Value),
}

impl IrGenResult {
    /// Get the value if it is a value.
    ///
    /// # Panics
    /// - Panics if it is a global.
    pub fn unwrap_value(self) -> Value {
        match self {
            IrGenResult::Value(val) => val,
            IrGenResult::Global(_) => unreachable!("expected value"),
        }
    }
}

/// IR generation context.
#[derive(Default)]
pub struct IrGenContext {
    pub ctx: Context,

    // Symbol table
    pub symtable: SymbolTable,

    // Current function and block
    pub curr_func: Option<Func>,
    pub curr_func_name: Option<String>,
    pub curr_block: Option<Block>,

    // Stacks for loop control flow.
    pub loop_entry_stack: Vec<Block>,
    pub loop_exit_stack: Vec<Block>,

    // Return block and slot
    pub curr_ret_slot: Option<Value>,
    pub curr_ret_block: Option<Block>,
}

impl IrGenContext {
    /// Consume the context and return the generated IR.
    pub fn finish(self) -> Context { self.ctx }

    // Generate a new global constant value in ir given a comptime value in AST.
    fn gen_global_comptime(&mut self, val: &Cv) -> ConstantValue {
        match val {
            Cv::Bool(a) => ConstantValue::i1(&mut self.ctx, *a),
            Cv::Int(a) => ConstantValue::i32(&mut self.ctx, *a),
            Cv::Undef(ty) => {
                let ir_ty = self.gen_type(ty);
                ConstantValue::undef(&mut self.ctx, ir_ty)
            }
        }
    }

    // Gerate a new type in ir given a type in AST.
    fn gen_type(&mut self, ty: &Type) -> Ty {
        match ty.kind() {
            Tk::Void => Ty::void(&mut self.ctx),
            Tk::Bool => Ty::i1(&mut self.ctx),
            Tk::Int => Ty::i32(&mut self.ctx),
            Tk::Func(..) => unreachable!("function type should be handled separately"),
        }
    }

    // Generate a new local constant value in ir given a comptime value in AST.
    fn gen_local_comptime(&mut self, val: &Cv) -> Value {
        match val {
            Cv::Bool(a) => Value::i1(&mut self.ctx, *a),
            Cv::Int(a) => Value::i32(&mut self.ctx, *a),
            Cv::Undef(ty) => {
                let ir_ty = self.gen_type(ty);
                Value::undef(&mut self.ctx, ir_ty)
            }
        }
    }

    fn map_int_binary_op(&self, op: &BinaryOp) -> ir::IntBinaryOp {
        use BinaryOp as Bo;
        // let ir_int_binary_op =
        match op {
            Bo::Add => ir::IntBinaryOp::Add,
            Bo::Sub => ir::IntBinaryOp::Sub,
            Bo::Mul => ir::IntBinaryOp::Mul,
            Bo::Div => ir::IntBinaryOp::SDiv,
            Bo::Mod => ir::IntBinaryOp::SRem,
            Bo::Lt => ir::IntBinaryOp::ICmp {
                cond: ir::IntCmpCond::Slt,
            },
            Bo::Gt => ir::IntBinaryOp::ICmp {
                cond: ir::IntCmpCond::Sgt,
            },
            Bo::Le => ir::IntBinaryOp::ICmp {
                cond: ir::IntCmpCond::Sle,
            },
            Bo::Ge => ir::IntBinaryOp::ICmp {
                cond: ir::IntCmpCond::Sge,
            },
            Bo::Eq => ir::IntBinaryOp::ICmp {
                cond: ir::IntCmpCond::Eq,
            },
            Bo::Ne => ir::IntBinaryOp::ICmp {
                cond: ir::IntCmpCond::Ne,
            },
            Bo::And => ir::IntBinaryOp::And,
            Bo::Or => ir::IntBinaryOp::Or,
        }
        // ir_int_binary_op
    }

    // Generate a new local expression in ir given an expression in AST.
    fn gen_local_expr(&mut self, expr: &Expr) -> Option<Value> {
        let curr_block = self.curr_block.unwrap();

        match &expr.kind {
            // Constants -> generate a local constant value
            ExprKind::Const(v) => Some(self.gen_local_comptime(v)),
            // Binary operations -> generate the operation
            ExprKind::Binary(op, lhs, rhs) => {
                let lhs = self.gen_local_expr(lhs).unwrap(); // Generate lhs
                let rhs = self.gen_local_expr(rhs).unwrap(); // Generate rhs

                let lhs_ty = lhs.ty(&self.ctx);
                let is_float = lhs_ty.is_float(&self.ctx);

                let ir_int_binary_op = self.map_int_binary_op(op);

                if is_float {
                    todo!("implement float binary!");
                } else {
                    let inst = Inst::ibinary(&mut self.ctx, ir_int_binary_op, lhs, rhs);
                    // Push the instruction to the current block
                    curr_block.push_back(&mut self.ctx, inst).unwrap();
                    Some(inst.result(&self.ctx).unwrap())
                }
            }
            // Unary operations -> generate the operation
            ExprKind::Unary(op, _) => match op {
                // TODO: Implement unary operations
                UnaryOp::Neg => {
                    todo!("implement neg");
                }
                UnaryOp::Not => {
                    todo!("implement not");
                }
            },
            // LValues -> Get the value
            ExprKind::LVal(LVal { ident }) => {
                // Look up the symbol in the symbol table to get the IR value
                let entry = self.symtable.lookup(ident).unwrap();
                let ir_value = entry.ir_value.unwrap();

                let ir_base_ty = self.gen_type(&entry.ty.clone());

                let slot = if let IrGenResult::Global(slot) = ir_value {
                    // If the value is a global, get the global reference
                    let name = slot.name(&self.ctx).to_string();
                    let value_ty = slot.ty(&self.ctx);
                    Value::global_ref(&mut self.ctx, name, value_ty)
                } else if let IrGenResult::Value(slot) = ir_value {
                    // If the value is a local, get the value
                    slot
                } else {
                    unreachable!()
                };

                if slot.is_param(&self.ctx) {
                    // If the value is a parameter, just return the value
                    Some(slot)
                } else {
                    // Otherwise, we need to load the value, generate a load instruction
                    let load = Inst::load(&mut self.ctx, slot, ir_base_ty);
                    curr_block.push_back(&mut self.ctx, load).unwrap();
                    Some(load.result(&self.ctx).unwrap())
                }
            }
            ExprKind::Coercion(_) => {
                // TODO: Implement coercion generation
                todo!("implement coercion");
            }
            ExprKind::FuncCall(FuncCall { .. }) => {
                // TODO: Implement function call generation
                todo!("implement call");
            }
        }
    }

    // Generate the system library function definitions.
    fn gen_sysylib(&mut self) {
        // TODO: Implement gen_sysylib
        // Since the system library is linked in the linking phase, we just need
        // to generate declarations here.
    }
}

pub trait IrGen {
    fn irgen(&self, irgen: &mut IrGenContext);
}

impl IrGen for CompUnit {
    // Generate IR for the compilation unit.
    fn irgen(&self, irgen: &mut IrGenContext) {
        // Enter the global scope
        irgen.symtable.enter_scope();
        // Generate system library function definitions
        irgen.gen_sysylib();
        // Generate IR for each item in the compilation unit
        for item in &self.items {
            item.irgen(irgen);
        }
        // Leave the global scope
        irgen.symtable.leave_scope();
    }
}

impl IrGen for Item {
    // Generate IR for an item.
    fn irgen(&self, irgen: &mut IrGenContext) {
        match self {
            Item::Decl(decl) => match decl {
                Decl::ConstDecl(ConstDecl { defs, .. }) => {
                    for ConstDef { ident, init, .. } in defs {
                        // Try to fold the initializer to get the constant value
                        // Note for const declaration, the initializer must be a constant
                        let comptime = init
                            .try_fold(&irgen.symtable)
                            .expect("global def expected to have constant initializer");
                        // Generate the constant value in IR
                        let constant = irgen.gen_global_comptime(&comptime);
                        let slot = Global::new(
                            &mut irgen.ctx,
                            format!("__GLOBAL_CONST_{}", ident),
                            constant,
                        );
                        // Insert the symbol in the symbol table
                        irgen.symtable.insert(
                            ident.clone(),
                            SymbolEntry {
                                ty: init.ty().clone(),
                                comptime: Some(comptime),
                                ir_value: Some(IrGenResult::Global(slot)),
                            },
                        );
                    }
                }
                Decl::VarDecl(VarDecl { defs, .. }) => {
                    for VarDef { ident, init, .. } in defs {
                        // Note that if the variable is defined without an initializer, aka,
                        // Undefined, we should already assigned their init as `None` in type
                        // checking phase.
                        let comptime = init
                            .as_ref()
                            .unwrap() // Safe to unwrap since we already checked it in type checking phase
                            .try_fold(&irgen.symtable)
                            .expect("global def expected to have constant initializer");
                        // Generate the constant value in IR
                        let constant = irgen.gen_global_comptime(&comptime);
                        let slot = Global::new(
                            &mut irgen.ctx,
                            format!("__GLOBAL_VAR_{}", ident),
                            constant,
                        );
                        // Insert the symbol in the symbol table
                        irgen.symtable.insert(
                            ident.clone(),
                            SymbolEntry {
                                ty: init.as_ref().unwrap().ty().clone(),
                                comptime: Some(comptime),
                                ir_value: Some(IrGenResult::Global(slot)),
                            },
                        );
                    }
                }
            },
            Item::FuncDef(func_def) => func_def.irgen(irgen),
        }
    }
}

impl IrGen for FuncDef {
    fn irgen(&self, irgen: &mut IrGenContext) {
        irgen.symtable.enter_scope();

        let mut param_tys = Vec::new();
        for FuncFParam { ty, .. } in self.params.iter() {
            param_tys.push(ty.clone());
        }

        let func_ty = Type::func(param_tys.clone(), self.ret_ty.clone());

        let ir_ret_ty = irgen.gen_type(&self.ret_ty);
        let func = Func::new(&mut irgen.ctx, self.ident.clone(), ir_ret_ty);

        irgen.symtable.insert_upper(
            self.ident.clone(),
            SymbolEntry {
                ty: func_ty,
                comptime: None,
                ir_value: None,
            },
            1,
        );

        let block = Block::new(&mut irgen.ctx);
        func.push_back(&mut irgen.ctx, block).unwrap();

        irgen.curr_func = Some(func);
        irgen.curr_func_name = Some(self.ident.clone());
        irgen.curr_block = Some(block);

        // block params
        for (FuncFParam { ident, .. }, ty) in self.params.iter().zip(param_tys.iter()) {
            let ir_ty = irgen.gen_type(ty);
            let param = func.add_param(&mut irgen.ctx, ir_ty);

            irgen.symtable.insert(
                ident.clone(),
                SymbolEntry {
                    ty: ty.clone(),
                    comptime: None,
                    ir_value: Some(IrGenResult::Value(param)),
                },
            );
        }

        // create slots for pass-by-value params
        for (FuncFParam { ident, .. }, ty) in self.params.iter().zip(param_tys.iter()) {
            if ty.is_int() {
                let ir_ty = irgen.gen_type(ty);
                let slot = Inst::alloca(&mut irgen.ctx, ir_ty);

                block.push_front(&mut irgen.ctx, slot).unwrap();
                let slot = slot.result(&irgen.ctx).unwrap();

                // get old entry
                let param = irgen
                    .symtable
                    .lookup(ident)
                    .unwrap()
                    .ir_value
                    .unwrap()
                    .unwrap_value();

                // store
                let store = Inst::store(&mut irgen.ctx, param, slot);

                block.push_back(&mut irgen.ctx, store).unwrap();

                // set new entry
                irgen.symtable.insert(
                    ident.clone(),
                    SymbolEntry {
                        ty: ty.clone(),
                        comptime: None,
                        ir_value: Some(IrGenResult::Value(slot)),
                    },
                );
            }
        }

        // create return block and slot
        let ret_block = Block::new(&mut irgen.ctx);
        irgen.curr_ret_block = Some(ret_block);

        if !self.ret_ty.is_void() {
            let ir_ret_ty = irgen.gen_type(&self.ret_ty);
            let ret_slot = Inst::alloca(&mut irgen.ctx, ir_ret_ty);

            block.push_front(&mut irgen.ctx, ret_slot).unwrap();
            irgen.curr_ret_slot = Some(ret_slot.result(&irgen.ctx).unwrap());
        }

        // generate body
        self.body.irgen(irgen);

        // append return block
        func.push_back(&mut irgen.ctx, ret_block).unwrap();

        if !self.ret_ty.is_void() {
            // load, ret
            let ret_slot = irgen.curr_ret_slot.unwrap();
            let ty = irgen.gen_type(&self.ret_ty);

            let load = Inst::load(&mut irgen.ctx, ret_slot, ty);
            ret_block.push_back(&mut irgen.ctx, load).unwrap();
            let val = load.result(&irgen.ctx).unwrap();

            let ret = Inst::ret(&mut irgen.ctx, Some(val));
            ret_block.push_back(&mut irgen.ctx, ret).unwrap();
        } else {
            // just return
            let ret = Inst::ret(&mut irgen.ctx, None);
            ret_block.push_back(&mut irgen.ctx, ret).unwrap();
        }

        irgen.curr_func = None;
        irgen.curr_func_name = None;
        irgen.curr_block = None;
        irgen.curr_ret_slot = None;
        irgen.curr_ret_block = None;

        irgen.symtable.leave_scope();
    }
}

impl IrGen for Decl {
    fn irgen(&self, irgen: &mut IrGenContext) {
        let entry_block = irgen.curr_func.unwrap().head(&irgen.ctx).unwrap();
        let curr_block = irgen.curr_block.unwrap();
        match self {
            Decl::ConstDecl(ConstDecl { defs, .. }) => {
                for ConstDef { ident, init, .. } in defs {
                    let comptime = init
                        .try_fold(&irgen.symtable)
                        .expect("global def expected to have constant initializer");

                    let ir_ty = irgen.gen_type(init.ty());
                    let stack_slot = Inst::alloca(&mut irgen.ctx, ir_ty);

                    entry_block.push_front(&mut irgen.ctx, stack_slot).unwrap();
                    irgen.symtable.insert(
                        ident,
                        SymbolEntry {
                            ty: init.ty().clone(),
                            comptime: Some(comptime),
                            ir_value: Some(IrGenResult::Value(
                                stack_slot.result(&irgen.ctx).unwrap(),
                            )),
                        },
                    );
                    let init = irgen.gen_local_expr(init).unwrap();
                    let slot = stack_slot.result(&irgen.ctx).unwrap();
                    let store = Inst::store(&mut irgen.ctx, init, slot);
                    curr_block.push_back(&mut irgen.ctx, store).unwrap();
                }
            }
            Decl::VarDecl(VarDecl { defs, .. }) => {
                for VarDef { ident, init, .. } in defs {
                    let init = init.as_ref().unwrap();
                    let ir_ty = irgen.gen_type(init.ty());
                    let stack_slot = Inst::alloca(&mut irgen.ctx, ir_ty);

                    entry_block.push_front(&mut irgen.ctx, stack_slot).unwrap();
                    irgen.symtable.insert(
                        ident,
                        SymbolEntry {
                            ty: init.ty().clone(),
                            comptime: None,
                            ir_value: Some(IrGenResult::Value(
                                stack_slot.result(&irgen.ctx).unwrap(),
                            )),
                        },
                    );

                    let init = irgen.gen_local_expr(init).unwrap();
                    let slot = stack_slot.result(&irgen.ctx).unwrap();
                    let store = Inst::store(&mut irgen.ctx, init, slot);
                    curr_block.push_back(&mut irgen.ctx, store).unwrap();
                }
            }
        }
    }
}

impl IrGen for Stmt {
    fn irgen(&self, irgen: &mut IrGenContext) {
        let curr_block = irgen.curr_block.unwrap();

        match self {
            Stmt::Assign(LVal { ident }, expr) => {
                let entry = irgen.symtable.lookup(ident).unwrap();
                let ir_value = entry.ir_value.unwrap();

                let slot = if let IrGenResult::Global(slot) = ir_value {
                    let name = slot.name(&irgen.ctx).to_string();
                    let value_ty = slot.ty(&irgen.ctx);
                    Value::global_ref(&mut irgen.ctx, name, value_ty)
                } else if let IrGenResult::Value(slot) = ir_value {
                    slot
                } else {
                    unreachable!()
                };

                let store_dst = slot;

                let val = irgen.gen_local_expr(expr).unwrap();
                let store = Inst::store(&mut irgen.ctx, val, store_dst);
                curr_block.push_back(&mut irgen.ctx, store).unwrap();
            }
            Stmt::Expr(ExprStmt { expr }) => {
                if let Some(ref expr) = expr {
                    irgen.gen_local_expr(expr);
                }
            }
            Stmt::Block(block) => block.irgen(irgen),
            Stmt::If(..) => {
                todo!("implement if statement");
            }
            Stmt::While(..) => {
                todo!("implement while statement");
            }
            Stmt::Break => {
                todo!("implement break statement");
            }
            Stmt::Continue => {
                todo!("implement continue statement");
            }
            Stmt::Return(ReturnStmt { expr }) => {
                if let Some(expr) = expr {
                    let val = irgen.gen_local_expr(expr).unwrap();
                    let store = Inst::store(&mut irgen.ctx, val, irgen.curr_ret_slot.unwrap());
                    irgen
                        .curr_block
                        .unwrap()
                        .push_back(&mut irgen.ctx, store)
                        .unwrap();
                }

                let jump = Inst::br(&mut irgen.ctx, irgen.curr_ret_block.unwrap());
                irgen
                    .curr_block
                    .unwrap()
                    .push_back(&mut irgen.ctx, jump)
                    .unwrap();
            }
        }
    }
}

impl IrGen for ast::Block {
    fn irgen(&self, irgen: &mut IrGenContext) {
        irgen.symtable.enter_scope();
        for item in self.items.iter() {
            match item {
                BlockItem::Decl(decl) => decl.irgen(irgen),
                BlockItem::Stmt(stmt) => stmt.irgen(irgen),
            }
        }
        irgen.symtable.leave_scope();
    }
}
