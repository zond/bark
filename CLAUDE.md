# Agent Instructions

Always give the user your honest and carefully considered opinion. Never assume something is
correct or positive just because the user claims so or seems to want it. Trust the user, but
verify their statements when possible. Question what seems incorrect, impractical or
unfounded, and say what the tradeoff is in terms of concepts and ideals, not only
"algorithm A or B".

Run `date` to know what day it is.

Read README.md first. The protocol specification lives outside this repo:
`~/projects/hrafnar/docs/civilian-433-mesh-design.md`. The Darkmap routing design it
reduces is `~/projects/darkforest/docs/darkmap.md` and the code is
`~/projects/darkforest/darkmap`. We copy from Darkmap; we do not depend on it (docs/roadmap.md,
decision D1).

## Design ideals

Principled, simple, elegant and correct design and code is paramount. Prefer physics-grounded
constants over magic numbers: when a threshold can be derived from the radio (symbol time,
sensitivity, datasheet switching times, regulatory limits), derive it and cite the source.
Prefer hard reality (what the SX1262 does, what a person with a phone does) over assumptions.

Never postpone technical debt. Architectural changes are fair game when correctness demands
them; "smallest diff" is not an ideal in itself.

Do not optimise before the algorithm is correct, simple and principled. Verify every
optimisation.

The simulator and the firmware run the same protocol core. If a behaviour cannot be
expressed in the core, it does not go in the firmware either.

Small, well-reviewed commits that each tell one story.

## Documentation ideals

README.md: overview and build instructions. CLAUDE.md: what an agent needs that README does
not say. docs/: design notes, research, decisions, with dates. Record decisions in
docs/roadmap.md with a D-number so they can be referred to and revisited.
