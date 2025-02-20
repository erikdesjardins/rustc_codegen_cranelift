use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use cranelift::codegen::{
    entity::SecondaryMap,
    ir::{self, entities::AnyEntity, function::DisplayFunctionAnnotations},
    write::{FuncWriter, PlainWriter},
    ValueLabelsRanges,
};

use crate::prelude::*;

/// This module provides the [CommentWriter] which makes it possible
/// to add comments to the written cranelift ir.
///
/// # Example
///
/// ```clif
/// test compile
/// target x86_64
///
/// function u0:0(i64, i64, i64) system_v {
/// ; symbol _ZN119_$LT$example..IsNotEmpty$u20$as$u20$mini_core..FnOnce$LT$$LP$$RF$$u27$a$u20$$RF$$u27$b$u20$$u5b$u16$u5d$$C$$RP$$GT$$GT$9call_once17he85059d5e6a760a0E
/// ; instance Instance { def: Item(DefId(0/0:29 ~ example[8787]::{{impl}}[0]::call_once[0])), substs: [ReErased, ReErased] }
/// ; sig ([IsNotEmpty, (&&[u16],)]; c_variadic: false)->(u8, u8)
///
/// ; ssa {_2: NOT_SSA, _4: NOT_SSA, _0: NOT_SSA, _3: (empty), _1: NOT_SSA}
/// ; msg   loc.idx    param    pass mode            ssa flags  ty
/// ; ret    _0      = v0       ByRef                NOT_SSA    (u8, u8)
/// ; arg    _1      = v1       ByRef                NOT_SSA    IsNotEmpty
/// ; arg    _2.0    = v2       ByVal(types::I64)    NOT_SSA    &&[u16]
///
///     ss0 = explicit_slot 0 ; _1: IsNotEmpty size=0 align=1,8
///     ss1 = explicit_slot 8 ; _2: (&&[u16],) size=8 align=8,8
///     ss2 = explicit_slot 8 ; _4: (&&[u16],) size=8 align=8,8
///     sig0 = (i64, i64, i64) system_v
///     sig1 = (i64, i64, i64) system_v
///     fn0 = colocated u0:6 sig1 ; Instance { def: Item(DefId(0/0:31 ~ example[8787]::{{impl}}[1]::call_mut[0])), substs: [ReErased, ReErased] }
///
/// ebb0(v0: i64, v1: i64, v2: i64):
///     v3 = stack_addr.i64 ss0
///     v4 = stack_addr.i64 ss1
///     store v2, v4
///     v5 = stack_addr.i64 ss2
///     jump ebb1
///
/// ebb1:
///     nop
/// ; _3 = &mut _1
/// ; _4 = _2
///     v6 = load.i64 v4
///     store v6, v5
/// ;
/// ; _0 = const mini_core::FnMut::call_mut(move _3, move _4)
///     v7 = load.i64 v5
///     call fn0(v0, v3, v7)
///     jump ebb2
///
/// ebb2:
///     nop
/// ;
/// ; return
///     return
/// }
/// ```

#[derive(Debug)]
pub struct CommentWriter {
    global_comments: Vec<String>,
    entity_comments: HashMap<AnyEntity, String>,
    inst_comments: HashMap<Inst, String>,
}

impl CommentWriter {
    pub fn new<'tcx>(tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> Self {
        CommentWriter {
            global_comments: vec![
                format!("symbol {}", tcx.symbol_name(instance).name.as_str()),
                format!("instance {:?}", instance),
                format!(
                    "sig {:?}",
                    tcx.normalize_erasing_late_bound_regions(
                        ParamEnv::reveal_all(),
                        &instance.fn_sig(tcx)
                    )
                ),
                String::new(),
            ],
            entity_comments: HashMap::new(),
            inst_comments: HashMap::new(),
        }
    }
}

impl FuncWriter for &'_ CommentWriter {
    fn write_preamble(
        &mut self,
        w: &mut dyn fmt::Write,
        func: &Function,
        reg_info: Option<&isa::RegInfo>,
    ) -> Result<bool, fmt::Error> {
        for comment in &self.global_comments {
            if !comment.is_empty() {
                writeln!(w, "; {}", comment)?;
            } else {
                writeln!(w, "")?;
            }
        }
        if !self.global_comments.is_empty() {
            writeln!(w, "")?;
        }

        self.super_preamble(w, func, reg_info)
    }

    fn write_entity_definition(
        &mut self,
        w: &mut dyn fmt::Write,
        _func: &Function,
        entity: AnyEntity,
        value: &dyn fmt::Display,
    ) -> fmt::Result {
        write!(w, "    {} = {}", entity, value)?;

        if let Some(comment) = self.entity_comments.get(&entity) {
            writeln!(w, " ; {}", comment.replace('\n', "\n; "))
        } else {
            writeln!(w, "")
        }
    }

    fn write_ebb_header(
        &mut self,
        w: &mut dyn fmt::Write,
        func: &Function,
        isa: Option<&dyn isa::TargetIsa>,
        ebb: Ebb,
        indent: usize,
    ) -> fmt::Result {
        PlainWriter.write_ebb_header(w, func, isa, ebb, indent)
    }

    fn write_instruction(
        &mut self,
        w: &mut dyn fmt::Write,
        func: &Function,
        aliases: &SecondaryMap<Value, Vec<Value>>,
        isa: Option<&dyn isa::TargetIsa>,
        inst: Inst,
        indent: usize,
    ) -> fmt::Result {
        PlainWriter.write_instruction(w, func, aliases, isa, inst, indent)?;
        if let Some(comment) = self.inst_comments.get(&inst) {
            writeln!(w, "; {}", comment.replace('\n', "\n; "))?;
        }
        Ok(())
    }
}

#[cfg(debug_assertions)]
impl<'a, 'tcx, B: Backend + 'static> FunctionCx<'_, 'tcx, B> {
    pub fn add_global_comment<S: Into<String>>(&mut self, comment: S) {
        self.clif_comments.global_comments.push(comment.into());
    }

    pub fn add_entity_comment<'s, S: Into<Cow<'s, str>>, E: Into<AnyEntity>>(
        &mut self,
        entity: E,
        comment: S,
    ) {
        use std::collections::hash_map::Entry;
        match self.clif_comments.entity_comments.entry(entity.into()) {
            Entry::Occupied(mut occ) => {
                occ.get_mut().push('\n');
                occ.get_mut().push_str(comment.into().as_ref());
            }
            Entry::Vacant(vac) => {
                vac.insert(comment.into().into_owned());
            }
        }
    }

    pub fn add_comment<'s, S: Into<Cow<'s, str>>>(&mut self, inst: Inst, comment: S) {
        use std::collections::hash_map::Entry;
        match self.clif_comments.inst_comments.entry(inst) {
            Entry::Occupied(mut occ) => {
                occ.get_mut().push('\n');
                occ.get_mut().push_str(comment.into().as_ref());
            }
            Entry::Vacant(vac) => {
                vac.insert(comment.into().into_owned());
            }
        }
    }
}

pub fn write_clif_file<'tcx>(
    tcx: TyCtxt<'tcx>,
    postfix: &str,
    instance: Instance<'tcx>,
    func: &ir::Function,
    mut clif_comments: &CommentWriter,
    value_ranges: Option<&ValueLabelsRanges>,
) {
    use std::io::Write;

    let symbol_name = tcx.symbol_name(instance).name.as_str();
    let clif_file_name = format!(
        "{}/{}__{}.{}.clif",
        concat!(env!("CARGO_MANIFEST_DIR"), "/target/out/clif"),
        tcx.crate_name(LOCAL_CRATE),
        symbol_name,
        postfix,
    );

    let mut clif = String::new();
    cranelift::codegen::write::decorate_function(
        &mut clif_comments,
        &mut clif,
        &func,
        &DisplayFunctionAnnotations {
            isa: Some(&*crate::build_isa(
                tcx.sess, true, /* PIC doesn't matter here */
            )),
            value_ranges,
        },
    )
    .unwrap();

    match ::std::fs::File::create(clif_file_name) {
        Ok(mut file) => {
            let target_triple = crate::target_triple(tcx.sess);
            writeln!(file, "test compile").unwrap();
            writeln!(file, "set is_pic").unwrap();
            writeln!(file, "target {}", target_triple).unwrap();
            writeln!(file, "").unwrap();
            file.write(clif.as_bytes()).unwrap();
        }
        Err(e) => {
            tcx.sess.warn(&format!("err opening clif file: {:?}", e));
        }
    }
}

impl<'a, 'tcx, B: Backend + 'static> fmt::Debug for FunctionCx<'_, 'tcx, B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{:?}", self.instance.substs)?;
        writeln!(f, "{:?}", self.local_map)?;

        let mut clif = String::new();
        ::cranelift::codegen::write::decorate_function(
            &mut &self.clif_comments,
            &mut clif,
            &self.bcx.func,
            &DisplayFunctionAnnotations::default(),
        )
        .unwrap();
        writeln!(f, "\n{}", clif)
    }
}
