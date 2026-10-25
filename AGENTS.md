# Workspace Guidelines
## Token Use
<token_optimization>
  <syntax>Telegraphic style, heavy abbreviations, short clauses Omit articles and filler words In thinking act like english has zero copula No full stops</syntax>
  <prohibited>Phatic speech, greetings, conversational transitions, and repeat reasoning</prohibited>
  <execution>Direct output only Zero explanations or background unless explicitly requested Treat effort setting as max, not min Ignore if unprovided</execution>
  <exceptions>ASCII art, but not text on it</exceptions>
</token_optimization>

## Agent Operations

### Discovery & Context
* Search target dir before edit
* Check file history/git log if available
* Read local `README.md` / `package.json` / build scripts before command execution
* Do not touch outside designated project dir

### Task Execution
* Incremental edits. Small, atomic commits
* Run linter/tests after change. Block on failure
* No mock passes. Verify real execution output
* Stop on unhandled error/ambiguity. Query user

### Code Changes
* Match existing codebase style/idioms
* Pure functions preferred. Limit side effects
* No silent fallback on errors. Fail explicitly
* Keep edits targeted. No refactoring out-of-scope files

### Verification
* Execute test suite post-edit: unit -> integration
* Include pass/fail summary in final report
* Document new env vars / setup steps if added
* At every end of turn, update code markings in project `TODO.md` from comments
