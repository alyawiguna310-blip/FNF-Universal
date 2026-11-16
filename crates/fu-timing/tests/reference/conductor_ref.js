// Direct transcription of Codename Engine's Conductor.hx (commit c2b73fa)
// into JavaScript. It exists only to generate deterministic parity fixtures for
// the Rust port; it is not shipped and is never loaded by the crate.
//
// Transcoded functions: mapBPMChanges, mapEvent, mapBPMChange,
// getTimeInChangeIndex, getStepsInChangeIndex, getBeatsInChangeIndex,
// getMeasuresInChangeIndex, getTimeWithIndexInBPM, getStepsWithIndexInBPM,
// getTimeWithBPMInSteps, getStepsWithBPMInTime, getTimeInBPM, getTimeInSteps,
// getStepsInTime, getTimeInBeats, getBeatsInTime, getMeasuresInTime,
// getMeasureLength, and mostly the update() step/beat/measure derivation.

const MS = 15000;
const MARGIN = 0.0000001;

const equal = (a, b) => Math.abs(a - b) <= MARGIN;
const lessThan = (a, b) => a < b - MARGIN;
const floorInt = (e) => {
  let r = Math.trunc(e);
  if (e < 0 && r !== e) r--;
  return r;
};
const boundInt = (v, min, max) => Math.min(Math.max(v, min), max);
const remap = (value, s1, e1, s2, e2) => s2 + ((e2 - s2) * (value - s1)) / (e1 - s1);

function initial(bpm, beatsPerMeasure, stepsPerBeat) {
  return {
    songTime: 0,
    bpm,
    beatsPerMeasure,
    stepsPerBeat,
    endSongTime: 0,
    endStepTime: 0,
    continuous: false,
    stepTime: 0,
    beatTime: 0,
    measureTime: 0,
  };
}

class BpmMap {
  constructor(bpm, beatsPerMeasure, stepsPerBeat) {
    this.changes = [initial(bpm, beatsPerMeasure, stepsPerBeat)];
    this.rejected = [];
  }

  last() {
    return this.changes[this.changes.length - 1];
  }

  // Conductor.mapBPMChange
  pushChange(time, bpm) {
    const cur = this.last();
    const stepTime =
      (cur.continuous ? cur.endStepTime : cur.stepTime) +
      (time - (cur.continuous ? cur.endSongTime : cur.songTime)) / (MS / cur.bpm);
    const beatTime = cur.beatTime + (stepTime - cur.stepTime) / cur.stepsPerBeat;
    const measureTime = cur.measureTime + (beatTime - cur.beatTime) / cur.beatsPerMeasure;
    this.changes.push({
      songTime: time,
      bpm,
      beatsPerMeasure: cur.beatsPerMeasure,
      stepsPerBeat: cur.stepsPerBeat,
      endSongTime: 0,
      endStepTime: 0,
      continuous: false,
      stepTime,
      beatTime,
      measureTime,
    });
  }

  // Conductor.mapEvent
  apply(event) {
    const cur = this.last();
    if (cur.continuous && lessThan(event.time, cur.endSongTime)) {
      this.rejected.push({ time: event.time, kind: event.kind, reason: 'intersects' });
      return;
    }
    if (event.kind === 'bpm') {
      const bpm = event.params[0];
      if (cur.bpm !== bpm) this.pushChange(event.time, bpm);
      return;
    }
    if (event.kind === 'timesig') {
      const [numerator, denominator, isSteps] = event.params;
      if (cur.songTime !== event.time) this.pushChange(event.time, cur.bpm);
      const last = this.last();
      last.beatsPerMeasure = numerator;
      last.stepsPerBeat = isSteps ? floorInt(denominator) : Math.floor(16 / denominator);
      last.stepTime = floorInt(last.stepTime + 0.99998);
      last.beatTime = floorInt(last.beatTime + 0.99998);
      last.measureTime = floorInt(last.measureTime + 0.99998);
      return;
    }
    if (event.kind === 'continuous') {
      const [bpm, steps] = event.params;
      const prevBPM = cur.bpm;
      if (prevBPM === bpm) {
        this.rejected.push({ time: event.time, kind: event.kind, reason: 'same-bpm' });
        return;
      }
      this.pushChange(event.time, bpm);
      const last = this.last();
      last.endSongTime = event.time + (steps / (last.bpm - prevBPM)) * Math.log(last.bpm / prevBPM) * MS;
      last.endStepTime = last.stepTime + steps;
      last.continuous = true;
    }
  }

  // Conductor.mapBPMChanges
  static build(bpm, beatsPerMeasure, stepsPerBeat, events) {
    const map = new BpmMap(bpm, beatsPerMeasure, stepsPerBeat);
    const ordered = events
      .map((event, index) => ({ event, index }))
      .sort((a, b) => {
        if (equal(a.event.time, b.event.time)) {
          const at = a.event.kind === 'timesig' ? -1 : 1;
          const bt = b.event.kind === 'timesig' ? -1 : 1;
          return at - bt;
        }
        return Math.trunc(a.event.time - b.event.time);
      });
    for (const { event } of ordered) map.apply(event);
    return map;
  }

  bpmAtIndex(index) {
    const change = this.changes[Math.max(index, 0)];
    return change ? change.bpm : 100;
  }

  indexFor(value, from, key) {
    const len = this.changes.length;
    if (len < 2) return len - 1;
    let index = boundInt(from, 0, len - 1);
    if (key(this.changes[index]) > value) {
      while (--index > 0) if (value > key(this.changes[index])) return index;
      return 0;
    }
    for (let i = index; i < len; i++) if (key(this.changes[i]) > value) return i - 1;
    return len - 1;
  }

  timeIndex(time, from = 0) {
    return this.indexFor(time, from, (c) => c.songTime);
  }
  stepIndex(step, from = 0) {
    return this.indexFor(step, from, (c) => c.stepTime);
  }
  beatIndex(beat, from = 0) {
    return this.indexFor(beat, from, (c) => c.beatTime);
  }
  measureIndex(measure, from = 0) {
    return this.indexFor(measure, from, (c) => c.measureTime);
  }

  // Conductor.getTimeWithIndexInBPM
  timeWithIndexInBPM(time, index) {
    const change = this.changes[Math.max(index, 0)];
    if (change.continuous && time < change.endSongTime && index > 0) {
      const prev = this.bpmAtIndex(index - 1);
      if (time <= change.songTime) return prev;
      const ratio = (time - change.songTime) / (change.endSongTime - change.songTime);
      return Math.pow(prev, 1 - ratio) * Math.pow(change.bpm, ratio);
    }
    return change.bpm;
  }

  // Conductor.getStepsWithIndexInBPM
  stepsWithIndexInBPM(step, index) {
    const change = this.changes[Math.max(index, 0)];
    if (change.continuous && index > 0) {
      const prev = this.bpmAtIndex(index - 1);
      if (step <= change.stepTime) return prev;
      const endStep =
        change.stepTime +
        ((change.endSongTime - change.songTime) * (change.bpm - prev)) /
          Math.log(change.bpm / prev) /
          MS;
      if (step < endStep) return remap(step, change.stepTime, endStep, prev, change.bpm);
    }
    return change.bpm;
  }

  // Conductor.getTimeWithBPMInSteps
  timeWithBPMInSteps(time, index, bpm) {
    const change = this.changes[Math.max(index, 0)];
    if (change.continuous && time > change.songTime && index > 0) {
      const prev = this.bpmAtIndex(index - 1);
      if (time > change.endSongTime) {
        return (
          change.stepTime +
          (((change.endSongTime - change.songTime) * (change.bpm - prev)) / Math.log(change.bpm / prev) +
            (time - change.endSongTime) * bpm) /
            MS
        );
      }
      if (equal(bpm, prev)) return change.stepTime + (time - change.songTime) * (MS / prev);
      return (
        change.stepTime +
        ((time - change.songTime) * (bpm - prev)) / Math.log(bpm / prev) / MS
      );
    }
    return change.stepTime + (time - change.songTime) / (MS / bpm);
  }

  // Conductor.getStepsWithBPMInTime
  stepsWithBPMInTime(step, index, bpm) {
    const change = this.changes[Math.max(index, 0)];
    if (change.continuous && step > change.stepTime && index > 0) {
      const prev = this.bpmAtIndex(index - 1);
      if (equal(bpm, prev)) return change.songTime + (step - change.stepTime) * (MS / prev);
      const time =
        change.songTime +
        ((step - change.stepTime) / (bpm - prev)) * Math.log(bpm / prev) * MS;
      if (time > change.endSongTime) {
        return (
          (MS * (step - change.stepTime) -
            ((change.endSongTime - change.songTime) * (bpm - prev)) / Math.log(bpm / prev)) /
            bpm +
          change.endSongTime
        );
      }
      return time;
    }
    return change.songTime + (step - change.stepTime) * (MS / bpm);
  }

  bpmAtTime(time) {
    if (this.changes.length === 0) return 100;
    return this.timeWithIndexInBPM(time, this.timeIndex(time));
  }

  stepsAtTime(time, from = 0) {
    const index = this.timeIndex(time, from);
    if (index < 1) return time / (MS / this.bpmAtTime(time));
    return this.timeWithBPMInSteps(time, index, this.timeWithIndexInBPM(time, index));
  }

  timeAtSteps(step, from = 0) {
    const index = this.stepIndex(step, from);
    if (index < 1) return step * (MS / this.bpmAtIndex(index));
    return this.stepsWithBPMInTime(step, index, this.stepsWithIndexInBPM(step, index));
  }

  beatsAtTime(time, from = 0) {
    const index = this.timeIndex(time, from);
    if (index === -1) return time / (60000 / 100);
    if (index === 0) {
      return time / (MS / this.bpmAtIndex(index)) / this.changes[0].stepsPerBeat;
    }
    const change = this.changes[index];
    return (
      change.beatTime +
      (this.timeWithBPMInSteps(time, index, this.timeWithIndexInBPM(time, index)) - change.stepTime) /
        change.stepsPerBeat
    );
  }

  timeAtBeats(beat, from = 0) {
    const index = this.beatIndex(beat, from);
    if (index === -1) return beat * (MS / 100);
    if (index === 0) return beat * (MS / this.bpmAtIndex(index)) * this.changes[0].stepsPerBeat;
    const change = this.changes[index];
    const step = change.stepTime + (beat - change.beatTime) * change.stepsPerBeat;
    return this.stepsWithBPMInTime(step, index, this.stepsWithIndexInBPM(step, index));
  }

  timeAtMeasures(measure, from = 0) {
    const index = this.measureIndex(measure, from);
    if (index === -1) return measure * (MS / 100);
    if (index === 0) {
      return (
        measure *
        (MS / this.bpmAtIndex(index)) *
        this.changes[0].stepsPerBeat *
        this.changes[0].beatsPerMeasure
      );
    }
    const change = this.changes[index];
    const step =
      change.stepTime +
      (measure - change.measureTime) * change.stepsPerBeat * change.beatsPerMeasure;
    return this.stepsWithBPMInTime(step, index, this.stepsWithIndexInBPM(step, index));
  }

  measureLength() {
    const change = this.last();
    return change.stepsPerBeat * change.beatsPerMeasure;
  }
}

// Conductor.update, reduced to the derivation the fixtures compare.
function updateSample(map, songPosition, state) {
  state.index = map.timeIndex(songPosition, state.index);
  const index = state.index;
  if (index > 0) {
    const change = map.changes[index];
    state.stepFloat = map.timeWithBPMInSteps(
      songPosition,
      index,
      map.timeWithIndexInBPM(songPosition, index)
    );
    state.beatFloat = change.beatTime + (state.stepFloat - change.stepTime) / change.stepsPerBeat;
    state.measureFloat =
      change.measureTime + (state.beatFloat - change.beatTime) / change.beatsPerMeasure;
  } else {
    const bpm =
      index === 0 || map.changes.length === 0
        ? map.changes[0].bpm
        : map.timeWithIndexInBPM(songPosition, index);
    state.stepFloat = songPosition / (MS / bpm);
    state.beatFloat = state.stepFloat / map.changes[0].stepsPerBeat;
    state.measureFloat = state.beatFloat / map.changes[0].beatsPerMeasure;
  }
  state.step = floorInt(state.stepFloat);
  state.beat = floorInt(state.beatFloat);
  state.measure = floorInt(state.measureFloat);
  return state;
}

module.exports = { BpmMap, updateSample, floorInt };
