# Workspace Guidelines

This workspace hosts multiple vibecoding projects at once. Treat each project as an independent unit with its own directory, source, tests, documentation, configuration, and version control context where practical. Do not place unrelated project files at workspace root.

## Workspace Organization

- Work inside a clearly named subdirectory for each project.
- Keep project-specific instructions, dependencies, commands, and documentation inside that project directory.
- Inspect the target project before editing; avoid changing neighboring projects.
- Use repository root only for shared workspace documentation or configuration.

## Token Use
<token_optimization>
  <syntax>Telegraphic style, heavy abbreviations, short clauses. Omit articles and filler words. In thinking act like english has zero copula.</syntax>
  <prohibited>Phatic speech, greetings, conversational transitions, and repeat reasoning.</prohibited>
  <execution>Direct output only. Zero explanations or background unless explicitly requested. Treat effort setting as max, not min. Ignore if unprovided.</execution>
  <exceptions>ASCII art, but not text on it.</exceptions>
</token_optimization>

## Project Structure

This checkout currently contains no application source, tests, assets, or project configuration. Add new code in clearly named, language-appropriate directories (for example, `src/` for implementation and `tests/` for automated tests). Keep documentation and configuration at the repository root when that is the convention for the chosen toolchain. Update this guide when the project structure is established.

## Build, Test, and Development

No build system or development commands are configured yet. When adding a toolchain, provide reproducible setup, run, build, and test commands in its README or package scripts, and document the canonical commands here. Run relevant checks before submitting changes; do not introduce a command that assumes unavailable local services or credentials without documenting the requirement.

## Coding Style and Naming

Follow the conventions of the language and framework selected for each module. Use consistent formatting, descriptive names, and the project’s formatter or linter once configured. Prefer names that reflect purpose; use the established naming pattern within each directory rather than introducing a competing style.

## Testing

There is no test framework or coverage threshold configured. Add tests alongside the chosen stack’s standard test layout, name them to describe the behavior under test, and document how to run them. Include regression coverage for bug fixes and focused tests for new behavior.

## Commits and Pull Requests

Git history is unavailable in this checkout, so no repository-specific commit convention can be inferred. Use short, imperative commit subjects (for example, `Add input validation`) until a team convention is established. Pull requests should explain the change and its rationale, list relevant verification commands and results, and link related issues. Include screenshots for visible UI changes.

Commit frequently. Make small, focused commits after each coherent change or working milestone. Keep unrelated project changes in separate commits. Use short, imperative commit subjects.

## Configuration and Secrets

Do not commit credentials, tokens, or machine-specific configuration. Document required environment variables with safe example values, and keep real secrets in the appropriate local or deployment secret store.
