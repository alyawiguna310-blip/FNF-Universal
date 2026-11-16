// Generates tests/fixtures/parity_cases.json from the reference transcription.
// Run: node crates/fu-timing/tests/reference/generate_parity.js
const fs = require('fs');
const path = require('path');
const { BpmMap, updateSample } = require('./conductor_ref.js');

const outPath = path.join(__dirname, '..', 'fixtures', 'parity_cases.json');

const sampleTimes = [-250, 0, 1, 150, 599.999, 600, 600.001, 1500, 2999.5, 3000, 3000.5, 4500, 6000, 6000.5, 8499, 12000, 23999.25];

function buildCase(name, bpm, beatsPerMeasure, stepsPerBeat, events, samples = sampleTimes) {
  const map = BpmMap.build(bpm, beatsPerMeasure, stepsPerBeat, events);
  const state = { index: 0, stepFloat: 0, beatFloat: 0, measureFloat: 0, step: 0, beat: 0, measure: 0 };
  const sampleResults = [];
  for (const time of samples) {
    const fresh = updateSample(map, time, { ...state });
    sampleResults.push({
      time,
      index: fresh.index,
      bpm: map.bpmAtTime(time),
      stepFloat: fresh.stepFloat,
      beatFloat: fresh.beatFloat,
      measureFloat: fresh.measureFloat,
      step: fresh.step,
      beat: fresh.beat,
      measure: fresh.measure,
      stepsAtTime: map.stepsAtTime(time),
      timeAtSteps: map.timeAtSteps(map.stepsAtTime(time)),
      beatsAtTime: map.beatsAtTime(time),
      timeAtBeats: map.timeAtBeats(map.beatsAtTime(time)),
      timeAtMeasures: map.timeAtMeasures(fresh.measure),
    });
  }
  return {
    name,
    bpm,
    beatsPerMeasure,
    stepsPerBeat,
    events,
    expected: {
      changes: map.changes,
      rejected: map.rejected,
      samples: sampleResults,
      measureLength: map.measureLength(),
    },
  };
}

const cases = [];

cases.push(buildCase('constant-100', 100, 4, 4, []));
cases.push(buildCase('constant-180', 180, 4, 4, []));
cases.push(
  buildCase('bpm-changes', 100, 4, 4, [
    { time: 3000, kind: 'bpm', params: [200] },
    { time: 9000, kind: 'bpm', params: [150] },
  ])
);
cases.push(
  buildCase('time-signature', 100, 4, 4, [
    { time: 6000, kind: 'timesig', params: [3, 4, false] },
    { time: 12000, kind: 'timesig', params: [7, 8, false] },
    { time: 18000, kind: 'timesig', params: [5, 6, true] },
  ])
);
cases.push(
  buildCase('continuous', 100, 4, 4, [
    { time: 0, kind: 'continuous', params: [200, 8] },
    { time: 4000, kind: 'bpm', params: [120] },
  ])
);
cases.push(
  buildCase('continuous-reject', 120, 4, 4, [
    { time: 1000, kind: 'continuous', params: [240, 8] },
    { time: 1100, kind: 'bpm', params: [90] },
  ])
);
cases.push(
  buildCase('mixed-unsorted', 140, 3, 4, [
    { time: 5000, kind: 'bpm', params: [180] },
    { time: 2000, kind: 'bpm', params: [110] },
    { time: 2000, kind: 'timesig', params: [3, 4, false] },
    { time: 8000, kind: 'continuous', params: [90, 6] },
    { time: 1000, kind: 'bpm', params: [140] },
  ])
);

// Deterministic fuzz: LCG so the corpus is reproducible without a seed library.
let seed = 0x5eed1234;
function rnd() {
  seed = (seed * 1664525 + 1013904223) >>> 0;
  return seed / 0x100000000;
}
for (let caseIndex = 0; caseIndex < 40; caseIndex++) {
  const bpm = 60 + Math.round(rnd() * 240);
  const beatsPerMeasure = [2, 3, 4, 4, 4, 5, 6][Math.floor(rnd() * 7)];
  const stepsPerBeat = [3, 4, 4, 4, 6][Math.floor(rnd() * 5)];
  const eventCount = 1 + Math.floor(rnd() * 5);
  const events = [];
  for (let i = 0; i < eventCount; i++) {
    const time = Math.round(rnd() * 12000 * 8) / 8;
    const roll = rnd();
    if (roll < 0.45) {
      events.push({ time, kind: 'bpm', params: [60 + Math.round(rnd() * 240)] });
    } else if (roll < 0.75) {
      events.push({
        time,
        kind: 'timesig',
        params: [
          1 + Math.floor(rnd() * 7),
          [2, 3, 4, 6, 8, 16][Math.floor(rnd() * 6)],
          rnd() < 0.2,
        ],
      });
    } else {
      events.push({
        time,
        kind: 'continuous',
        params: [60 + Math.round(rnd() * 240), 1 + Math.round(rnd() * 15)],
      });
    }
  }
  const samples = [];
  for (let i = 0; i < 12; i++) samples.push(Math.round(rnd() * 20000 * 8) / 8);
  cases.push(buildCase(`fuzz-${caseIndex}`, bpm, beatsPerMeasure, stepsPerBeat, events, samples));
}

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify({ cases }, null, 1));
console.log(`wrote ${outPath} (${cases.length} cases)`);
