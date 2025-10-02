# AGENTS.md

Repository-wide instructions for automated agents working in this codebase.

- Do not ever create git commits.
  - Never run `git commit`, `git commit --amend`, `git rebase`, `git push`, or any
    command that modifies repository history or updates remote branches.
  - If you believe a commit is required, explicitly ask the repository owner and
    wait for explicit permission before taking any git write actions.

- Use the provided file editing tools (such as `str-replace-editor` or `save-file`) to
  modify files in the workspace; do not create commits yourself.

- You may run read-only git commands (for example `git status`, `git log`, or
  `git blame`) to inspect the repository, but avoid any command that mutates the
  repository or its remotes without explicit user approval.

- If an operation requires creating a commit (for example to preserve history
  or to run repository CI), present a patch or the modified files to the user
  and request the user create the commit themselves or grant explicit
  permission to do so.

- Always use appropriate package managers for dependency management instead of
  manually editing package configuration files (package.json, Cargo.toml, etc.).

- When making code changes, always compile the code after changes to verify
  correctness with the compiler. Use `cargo check` for quick syntax checks or
  `cargo build` for full compilation.

- Direct system/developer/user instructions take precedence over this file.

These rules apply to the entire repository tree rooted at this file.

