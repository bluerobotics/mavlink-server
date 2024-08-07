use vergen_gix::{BuildBuilder, CargoBuilder, DependencyKind, GixBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    vergen_gix::Emitter::default()
        .add_instructions(&BuildBuilder::all_build()?)?
        .add_instructions(&GixBuilder::all_git()?)?
        .add_instructions(
            CargoBuilder::all_cargo()?.set_dep_kind_filter(Some(DependencyKind::Normal)),
        )?
        .emit()?;
    Ok(())
}
