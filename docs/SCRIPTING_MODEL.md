# Mod Scripting Model

## Authoring layers

1. Declarative data for common changes (note appearance, speed presets, settings).
2. Event scripts for song events, note lifecycle, enemy phases and cutscene events.
3. Advanced per-frame callbacks only where needed.

This keeps simple mods easy to make while allowing advanced creators to customize mechanics.

## Conceptual callbacks

These are API design examples, not implemented bindings:

```javascript
function onSongStart(context) {}
function onNoteSpawn(note, context) {}
function onNoteUpdate(note, deltaSeconds, context) {}
function onNoteHit(note, judgment, context) {}
function onNoteMiss(note, context) {}
function onEnemyPhase(phase, context) {}
function onSongEvent(event, context) {}
```

A script receives narrow, typed objects. It does not receive unrestricted access to the operating system, arbitrary files, process memory or network sockets.

## Visual movement example

```javascript
function onNoteUpdate(note, delta, context) {
  const t = context.songTime;
  note.visual.offsetX = Math.sin(t * 2.0) * 28;
  note.visual.rotation = Math.sin(t * 1.5) * 12;
}
```

The example changes only visual properties. It does not change lane identity, judgment time or hitbox.

## Difficulty

Difficulty names and presets are mod-defined. A mod can provide Easy, Normal, Hard, Expert or entirely custom labels. The engine validates the chart and exposes the chosen preset to scripts. It should not assume every mod has the same difficulty list.

## Script limits

The eventual runtime should enforce:
- per-callback instruction/time budget;
- memory budget;
- bounded entity creation;
- bounded particle/audio event rates;
- no arbitrary filesystem or process access;
- explicit permission declarations;
- deterministic behavior where practical;
- cancellation when a song or mod unloads.

Never run imported scripts during inspection or conversion. Static analysis and validation happen first.
