# Tauiler specifications

Tauiler is a proposed Rust-based tiling manager for Windows, inspired by Hyprland's cursor-directed Dwindle layout.

These documents describe the product before implementation. They distinguish agreed direction from proposals and unresolved decisions. A proposal is not an approved requirement.

## Documents

- [Product and scope](product.md): goals, boundaries, and proposed delivery stages.
- [Feature specifications](features.md): observable behavior and acceptance criteria.
- [Bindings](bindings.md): agreed shortcuts and proposed defaults.
- [Architecture](architecture.md): proposed Rust implementation and Windows integration.
- [Compatibility](compatibility.md): browser, Visual Studio, and File Explorer validation scenarios.
- [Decision log](decisions.md): agreed direction, constraints, and design questions.
- [Design interview](interview.md): the decision tree and current question round.

## Status vocabulary

- **Agreed:** explicitly established in the conversation.
- **Proposed:** recommended starting point, awaiting discussion.
- **Open:** a decision still required.
- **Constraint:** an operating-system limitation or implementation boundary.

Acceptance criteria describe intended verification. The native application is now implemented. See [implementation and validation](implementation.md) for delivered behavior, provisional defaults, executed checks, and remaining compatibility validation. See the [project README](../README.md) to build and run it.
