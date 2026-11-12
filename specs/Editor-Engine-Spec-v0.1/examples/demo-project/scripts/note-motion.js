// Conceptual SDK example. The runtime API is not implemented in this package.
function onNoteUpdate(note, deltaSeconds, context) {
  const t = context.songTime;
  note.visual.offsetX = Math.sin(t * 2.0 + note.seed) * 28.0;
  note.visual.rotation = Math.sin(t * 1.5 + note.seed) * 10.0;
}
