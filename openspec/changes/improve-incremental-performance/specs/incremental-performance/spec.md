# Incremental performance

## ADDED Requirements

### Requirement: Compilation reports identify expensive stages
The compiler SHALL preserve total elapsed reporting and separately report queue,
execution and diagnostic durations, project identity, revision and obsolescence.

#### Scenario: A queued compilation becomes obsolete
- **WHEN** newer inputs replace a queued snapshot before execution
- **THEN** expensive compilation is skipped
- **AND** in-flight accounting is released so the latest pending snapshot can run.

#### Scenario: Inputs change during compilation
- **WHEN** a running snapshot finishes after its inputs have changed
- **THEN** completion is acknowledged without publishing it as the current result
- **AND** the latest pending snapshot remains eligible to run.

### Requirement: Native theme previews follow viewer demand
Themed standalone preview SHALL update variants with live viewers and retain
current input state for variants without viewers.

#### Scenario: A viewer reconnects after edits
- **WHEN** a palette without viewers receives source updates and a viewer connects
- **THEN** that viewer eventually receives the current palette and revision.

#### Scenario: One of two same-palette viewers disconnects
- **WHEN** another viewer remains connected to the same palette
- **THEN** that palette continues to update.

### Requirement: Expensive results have bounded extended retention
Expensive pure byte results MAY survive normal automatic eviction within explicit
age, entry and payload-byte budgets. Manual clearing SHALL release them. Expired
values SHALL be destroyed outside cache write locks.

#### Scenario: Outer memoization temporarily hides an expensive inner call
- **WHEN** ordinary eviction ages an expensive pure byte result without a direct hit
- **THEN** extended retention preserves it while it fits the configured budget
- **AND** changed arguments still compute their own result.

### Requirement: CPU work declares its macOS scheduling priority
Native macOS workers SHALL distinguish interactive work from cache maintenance
using QoS and restore scoped changes when work completes. Performance observations
SHALL distinguish process memory from per-operation allocation claims.
