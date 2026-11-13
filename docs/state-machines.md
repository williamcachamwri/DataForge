# DataForge State Machines

All UI state machines live in `crates/dataforge-state` as pure-Rust, deterministic, headless-testable models.

## 1. `ConnectionWindowPhaseMachine` (`connection_window.rs`)

Manages the lifecycle of each connection workspace (`WorkspaceId`) inside a window:

```mermaid
stateDiagram-v2
    [*] --> Disconnected
    Disconnected --> Connecting: start_connect (new AttemptToken)
    Connecting --> Connected: complete(token matches)
    Connecting --> Unavailable: fail(token matches)
    Connecting --> Disconnected: cancel (advances token)
    Connected --> Reconnecting: start_reconnect (new AttemptToken)
    Connected --> Unavailable: connection_lost
    Connected --> Disconnected: disconnect
    Unavailable --> Reconnecting: retry (new AttemptToken)
    Reconnecting --> Connected: complete(token matches)
    Reconnecting --> Unavailable: fail(token matches)
```

- **`AttemptToken` Fencing**: Every connect/reconnect increments a monotonic `AttemptToken(u64)`. Cancelling updates the phase immediately and invalidates prior tokens so late driver callbacks cannot mutate state.
- **Inline `ConnectionUnavailableView`**: Failures render inline inside the workspace pane (`WorkspacePaneKind::UnavailableInline`) with `summary`, optional `detail`, `can_retry`, and `is_auth_failure` — never as modal alert dialogs.
- **Close & Safe Mode Guards**: `CloseDecision` checks `SessionLiveness` and open-transaction state before closing a workspace or window.
