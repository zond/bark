# Contributor guide

Rules for anyone, human or agent, who changes this repository.

## Working with the user

Give an honest, considered opinion. Do not treat a claim as correct because the user made
it or seems to want it. Verify what can be verified, question what looks wrong, and state
tradeoffs in terms of goals and principles, not only "option A or option B".

Run `date` to know the day.

## Where things are

Read `README.md` first. The protocol specification is `docs/protocol.md`; it is edited
together with the code it describes. Decisions are numbered D1, D2, ... in
`docs/roadmap.md` so they can be cited and revisited. The contract between the protocol core
and its drivers is `docs/core-interface.md`.

## Design

Simple, principled, correct design and code come first. Derive constants from the radio,
the datasheet or the regulation and cite the source; no magic numbers.

Prefer what the hardware and people actually do over assumptions.

Do not postpone technical debt. Change the architecture when correctness needs it;
"smallest diff" is not a goal.

Do not optimise before the algorithm is correct and simple. Verify every optimisation.

The simulator and the firmware run the same protocol core. If a behaviour cannot be
expressed in the core, it does not go in the firmware either.

The protocol core does not allocate and has no external routing dependency.

## Writing

Describe mechanisms in their own terms. Never name other projects as the origin of code or
ideas, in code, comments, docs or commit messages. A dated research note may cite another
project as the source of a fact, such as a pin assignment.

Documents describe what we do. Mention an alternative only when it is obvious or very
common and a reader would otherwise ask why not.

Comments say why, not what. The code says what. Keep comments as short as they can be while
still readable by a programmer who does not know this project.

Documents other than comments (`README.md`, this file, `docs/`) use plain English in plain
phrasing, ordinary words for things, and no project jargon. Use the fewest words that say
the thing clearly and without mystery.

Order a document so that each section builds only on the sections before it, and refer
back rather than forward wherever the order allows. Say each thing once, in the section
it belongs to, and refer to it from elsewhere. Write as tersely as a software or radio
engineer who has not seen the project can still read, and no terser.

`README.md` is the project overview. This file is the contributor guide and ruleset.
`docs/` holds design, research and decisions, each dated.

## Commits

Small, reviewed commits that each tell one story. Run `cargo test --workspace` and
`cargo clippy --workspace --all-targets` clean before each. Before committing, check that no
file names another project as a source.
