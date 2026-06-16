# Incan and Python compatibility

Incan uses Python-readable syntax where that helps authoring, but it is not a Python compatibility runtime because the goal is not to preserve CPython behavior; the goal is to make new application and data code easier to check, ship, and inspect.

Use this page when evaluating whether an existing Python habit, package, or semantic expectation carries over to Incan.

## Compatibility matrix

| Area                      | Python expectation                                                                                       | Incan position                                                                                                                                                        |
| ------------------------- | -------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Runtime                   | Source runs on CPython or another Python implementation.                                                 | Incan source is compiled by the Incan toolchain. There is no Python runtime process in the deployment model.                                                          |
| Existing packages         | `pip install` gives access to Python packages.                                                           | Python packages are not an Incan compatibility target. Use Python when package compatibility is required.                                                             |
| Syntax                    | Python syntax and grammar are preserved.                                                                 | Incan is Python-readable, not Python grammar compatible. Familiar forms are used where they fit the typed language.                                                   |
| Errors                    | Exceptions can be raised and caught dynamically.                                                         | Fallible paths should be visible through `Result`, `Option`, diagnostics, and explicit propagation.                                                                   |
| `None` and nullability    | `None` can appear at runtime unless discipline or typing tools catch it.                                 | Optionality should be modeled as `Option[T]` where the type system can enforce handling.                                                                              |
| Integers                  | Python integers are arbitrary precision by default.                                                      | Incan has ordinary integer spellings plus exact-width numeric types for data and interop boundaries. Width and conversion policy should be explicit where it matters. |
| Strings                   | Python `str` semantics and Unicode behavior are the compatibility baseline.                              | Incan keeps familiar string ergonomics, but string behavior is an Incan language contract, not a CPython compatibility promise.                                       |
| Mutability                | Many objects are mutable by convention, and mutation can be implicit in APIs.                            | Mutability should be visible in source and reviewable by the compiler.                                                                                                |
| Imports                   | Python import behavior, module search paths, and package initialization rules apply.                     | Incan modules, packages, manifests, and Rust interop imports follow Incan toolchain rules.                                                                            |
| Object model              | Python classes, metaclasses, descriptors, monkey patching, and dynamic attribute behavior are available. | Incan models, classes, traits, derives, and metadata are explicit typed constructs. Python's dynamic object model is not the target.                                  |
| Metaprogramming           | Runtime reflection and dynamic code generation are common escape hatches.                                | Incan favors checked metadata, derives, vocab/capability surfaces, and compiler-owned inspection facts.                                                               |
| Async                     | Python `asyncio` semantics and event loop integration define behavior.                                   | Incan async uses Incan source semantics and the current backend/runtime integration. It should not be assumed to be `asyncio` compatible.                             |
| Dataframes and notebooks  | Python ecosystem tools such as pandas, PySpark, notebooks, and ML libraries are directly usable.         | Incan's direction is typed data and domain packages such as InQL, not direct Python package execution.                                                                |
| Deployment                | A Python interpreter, environment, and package set are part of the runtime.                              | The deployment target is native artifacts plus package/build metadata.                                                                                                |
| C extension compatibility | CPython C extensions can be imported if the environment supports them.                                   | Incan does not target CPython extension compatibility. Use Rust interop or purpose-built Incan packages instead.                                                      |

## When Python compatibility tooling is the better answer

Use a Python compatibility or acceleration tool when:

- you already have Python code;
- Python package compatibility is mandatory;
- CPython behavior must be preserved;
- packaging an existing Python application is the goal;
- rewriting the public application surface is not acceptable.

Examples include Python runtimes, compilers, and extension tools such as RustPython, Codon, Nuitka, and Cython. Those are valuable projects, but they optimize for a different problem than Incan.

For the positive fit case, read [What Incan is for](../start_here/what_incan_is_for.md). This page stays narrower: it exists to answer whether Python habits and compatibility expectations carry over.
