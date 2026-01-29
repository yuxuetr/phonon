# Phonon Constitution (声子宪法)

**Effective Date**: 2026-01-12  
**Status**: Supreme Project Law

This document establishes the non-negotiable rules and core values for the Phonon project. All code, architecture decisions, and contributions must adhere to these principles.

## Article I. Stability & Safety (稳定性与安全性)

### Section 1. The "No Panic" Policy
1. **Absolute Ban**: The use of `.unwrap()` and `.expect()` is **strictly forbidden** in production code.
   - *Exception*: They are permitted in `#[test]` functions and example code (`examples/`) only.
2. **Error Handling**: All fallible operations must return `Result<T, E>`.
3. **Recovery**: The system must be able to recover gracefully from any audio processing error (e.g., corrupted files, device disconnection) without crashing the host application.

### Section 2. Type Safety
1. **Newtypes**: Use "Newtypes" pattern to enforce logic (e.g., `struct SampleRate(u32)` instead of raw `u32`) to prevent unit confusion.
2. **State Machines**: Use Rust's type system to make invalid states unrepresentable.

## Article II. Architecture & Design (架构与设计)

### Section 1. High Cohesion, Low Coupling (高内聚，低耦合)
1. **Single Responsibility**: Each crate and module must have a single, well-defined purpose.
   - `phonon-core`: Pure math/algorithms only. No I/O, no UI.
   - `phonon-engine`: Hardware interaction only.
2. **Dependency Direction**: Dependencies must flow from higher levels to lower levels. Core layers must never depend on UI or Engine layers.
3. **Interface Segregation**: Public APIs should be minimal. Expose only what is necessary.

### Section 2. Evolution & Scope (演进与范围)
1. **Iterative Development**: Do not build a "God Object" or "All-in-One" system from day one.
   - Start with WAV -> Then simple algos -> Then playback -> Then complex formats.
2. **MVP First**: Implement the Minimum Viable Product for each phase before optimizing or adding bells and whistles.
3. **YAGNI**: "You Aren't Gonna Need It". Do not over-engineer for hypothetical future requirements.

## Article III. Code Quality (代码质量)

### Section 1. Performance
1. **Zero-Cost Abstractions**: Prefer abstractions that compile away.
2. **Memory Awareness**: 
   - Audio processing is memory-intensive. Avoid unnecessary `.clone()`.
   - Prefer passing data by reference (`&[f32]`) over value.
   - Pre-allocate `Vec` capacity (`Vec::with_capacity`) when size is known.

### Section 2. Documentation & Testing
1. **Documentation**: All `pub` structs, enums, and functions must have `///` documentation comments explaining:
   - What it does.
   - Arguments and Return values.
   - **Panics**: Even if forbidden, if a panic is theoretically possible, it must be documented.
2. **Testing**:
   - **Unit Tests**: Required for all algorithmic logic.
   - **Integration Tests**: Required for crate-level public APIs.

## Article IV. Audio Philosophy (音频哲学)

1. **Universal Representation**: Internally, all audio is treated as `Vec<f32>` (floating point PCM).
2. **Format Agnosticism**: While Phase 1 focuses on WAV, the architecture must allow `AudioBuffer` to be populated from *any* source (MP3, FLAC, Network Stream, generated noise).
3. **Sample Accuracy**: Operations must be sample-accurate. Precision matters.

---
*Any violation of this Constitution in a Pull Request or Commit will result in immediate rejection.*
