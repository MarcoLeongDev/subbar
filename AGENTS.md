<!-- LIGHTSPEC:START -->
# LightSpec Instructions

IF (and only if) the user message:
  - Mentions planning or proposals (words like proposal, spec, change, plan)
  - Introduces new capabilities, breaking changes, architecture shifts, or big performance/security work
  - Sounds ambiguous and you need the authoritative spec before coding

THEN: 
Do the following:
- [ ] Suggest switching to `/plan` mode before writing the proposal if it's not already the case
- [ ] When the user approves the plan and terminates planning mode, run `lightspec-apply` to implement the approved proposal.
- [ ] Run `lightspec-proposal`.
- [ ] Do not implement the feature until the proposal is approved by the user. Implementation MUST be operated using `lightspec-apply` to ensure the implementation is properly tracked and documented.
- [ ] If unsure which skill to run, list installed skills.

Keep this managed block so 'lightspec update' can refresh the instructions.

<!-- LIGHTSPEC:END -->