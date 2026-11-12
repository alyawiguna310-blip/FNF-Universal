// Conceptual SDK example. The runtime API is not implemented in this package.
function onEnemyPhase(phase, context) {
  if (!context.settings.enemy_attacks) return;
  context.enemy.playAnimation(phase.animation);
}

function onNoteMiss(note, context) {
  if (!context.settings.enemy_attacks) return;
  const damage = context.settings.miss_damage;
  context.enemy.applyHealthChange(-damage);
}
