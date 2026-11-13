pub mod const_propagation;
pub mod dead_container_pruning;
pub mod layout_lowering;
pub mod lower_states;

use crate::ast::CuiFile;

/// Executes the complete CraftUI compiler optimization pipeline in order:
/// 1. `lower_states`: Lowers declarative `states` & `transitions` into reactive ternaries.
/// 2. `const_propagation`: Evaluates arithmetic, boolean logic, and static branches at compile time.
/// 3. `dead_container_pruning`: Strips useless container wrappers.
/// 4. `layout_lowering`: Canonicalizes properties and normalizes layout structures.
pub fn run_optimization_passes(file: &mut CuiFile) {
    lower_states::lower_states(file);
    const_propagation::propagate_constants(file);
    dead_container_pruning::prune_dead_containers(file);
    layout_lowering::lower_layout(file);
}
