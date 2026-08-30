# High-Assurance Refactoring, Security Hardening & Reliability Master Prompt

This document contains the master prompt and step-by-step instructions for performing modular, high-assurance refactoring, vulnerability reduction, and reliability improvements across the repository.

---

## How to Run This (1 Step / 1 Crate at a Time)

Instead of running everything in a single massive sweep, you can prompt the assistant to execute one phase or crate at a time:

```text
"Using HARDENING.md, execute Phase 1: Audit and report findings for [Crate / Subsystem Name] only."
```

Or for a specific component:
```text
"Using HARDENING.md, refactor and harden [crates/aequora-security] for edge cases, panic safety, and bound validation only."
```

---

## The Master Prompt

```markdown
You are an expert distributed systems and security engineer performing a comprehensive repository-wide audit, hardening, and refactoring pass on this codebase.

### Primary Objectives
1. **Security & Vulnerability Elimination**: Identify and eradicate any security risks, privilege escalations, tenant leaks, unsafe deserialization/decoding, unvalidated boundaries, secret leakage, replay vulnerabilities, or unbounded resource consumption (DoS/memory exhaustion).
2. **Reliability & Edge-Case Hardening**: Eliminate unhandled edge cases, subtle concurrency races, deadlocks, silent error swallows, unverified unwraps/panics, clock skew vulnerabilities, partition desynchronization, and partial state corruption.
3. **Clean Code & Architectural Refactoring**: Refactor redundant logic, simplify overcomplicated call chains, strengthen type-safety with newtypes/typestates, and ensure strict adherence to workspace architecture boundaries (e.g. Guppy layers, database neutrality, fail-closed contracts).
4. **Invariant & Contract Preservation**: Ensure all registered formal invariants, property tests, conformance suites, and verification gates continue to pass without weakening any guarantee.

---

### Step-by-Step Execution Workflow

#### Phase 1: Comprehensive Codebase & Threat Surface Audit
- Inspect all workspace crates across network boundaries, serialization paths, actor/tenant contexts, cryptographic operations, state engines, and storage abstractions.
- Audit every `unwrap()`, `expect()`, `unsafe` block, unchecked indexing (`[]`), infinite loop, recursive call, and bounded buffer allocation.
- Audit input validation: verify frame limits, batch limits, string length limits, recursion depth, and SSRF/redirect defenses for all external inputs.
- Audit multi-tenant isolation: verify that tenant IDs and authority epochs are server-validated, non-spoofable, and bound into every query, cursor, and mutation.

#### Phase 2: Implementation Plan & Strategy
- Group findings into:
  1. **Critical/High Security & Correctness fixes** (fail-closed authz, bounds checking, epoch fences, panic prevention).
  2. **Reliability & Resilience enhancements** (retries, timeouts, jitter, circuit breakers, idempotency keys, deterministic replay).
  3. **Architecture & Clean Code refactors** (removing duplication, unifying error enums, modularizing bloated modules, clarifying traits).
- Formulate an implementation plan ensuring changes are modular and backward-compatible.

#### Phase 3: Surgical Refactoring & Hardening
- Apply changes crate-by-crate following dependency order (foundational types -> domain logic -> facades -> CLI/tooling).
- Use strict type-level enforcement (e.g., parse-don't-validate, non-empty collections, sanitized wrapper types).
- Ensure every fallible operation returns a structured, typed domain error rather than generic or unhandled errors.
- Ensure logging and error paths redact credentials, tokens, sensitive tenant payloads, and private key material.

#### Phase 4: Edge-Case & Invariant Verification
- Run all verification scripts and gates:
  - Guppy crate boundary checks (`cargo run -q -p aequora-dev -- check`).
  - Database neutrality and isolation gates (`bash scripts/check-database-neutrality.sh`).
  - Domain-specific architecture gates (security, feed, registry, certification).
- Run full test suite, contract tests, property-based tests (proptest), and fuzz targets.
- Ensure 0 compiler warnings, 0 clippy warnings, and clean formatting.

#### Phase 5: Documentation & Evidence
- Document all vulnerabilities resolved, edge cases handled, and performance/clarity improvements made.
```

---

## Granular Command Prompts (1-by-1 Execution)

### Step 1: Audit Only
> "Read HARDENING.md Phase 1. Perform a thorough security and reliability audit of [crate/module name]. Do not edit code yet; list all findings, panic risks, boundary issues, and recommended fixes in a clear summary."

### Step 2: Implementation Plan
> "Based on the audit findings for [crate/module name], create a surgical implementation plan for refactoring, hardening error handling, and eliminating edge cases."

### Step 3: Refactor & Harden
> "Execute the approved plan for [crate/module name]. Ensure zero unwraps, explicit bounds checking, redactable secrets, and strict typing."

### Step 4: Verification
> "Run all contract tests, boundary checks, database neutrality gates, and property tests for [crate/module name] to verify complete correctness."