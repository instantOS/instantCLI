# Refactor

## Dependency Injection

### Current Implementation

Dependency injection is handled manually through function parameters, primarily passing references to `Config` (`&Config` or `&mut Config`) from `main.rs` to functions in the `dot` module. For example:

- `Config` is loaded once in `main()` and passed to functions like `apply_all(&config)`, `fetch_modified(&config, ...)`, etc.

- `LocalRepo` is created on-demand in functions using `LocalRepo::new(&config, name)`, extracting necessary data but not holding `Config` long-term.

- `Database` is created ad-hoc in multiple places (e.g., in `apply_all`, `fetch_modified`, `reset_modified`) via `Database::new()?`, which internally opens a SQLite connection using a fixed path from `config::db_path()`. The `Database` instance is then passed by reference (`&Database`) to `Dotfile` methods like `apply(&db)`, `fetch(&db)`.

This approach leverages Rust's ownership and borrowing system effectively for a small CLI application, avoiding global state and ensuring dependencies are explicit.

### Issues Identified

1. **Repeated Database Creation**: `Database::new()` is called multiple times across functions, each opening a new SQLite connection. This is inefficient (connection overhead) and could lead to issues like concurrent access problems if the app scales or runs in parallel.

2. **Tight Coupling**: `Database` is tightly coupled to the config module via hardcoded paths (`config::db_path()`). Changes to config paths require updates in `db.rs`.

3. **Lack of Centralized Management**: Shared resources like the database connection are not managed centrally, making it harder to ensure single-instance usage, connection pooling (if needed), or easy mocking for tests.

4. **Testability**: Ad-hoc creation makes unit testing harder; e.g., to test `Dotfile::apply`, you'd need to mock the entire `Database` creation flow.

### Proposed Improvements

1. **Create Database Once in Main**: Similar to Config, create a single `Database` instance in `main.rs` after loading Config, using the path from `config::db_path()`. Pass both `&config` and `&db` to functions that need them. This avoids repeated connections without introducing new structures.

   - Update top-level functions like `apply_all(&config, &db) -> Result<()> { ... }`.

   - Inside functions, pass `&db` to sub-calls as needed.

   - For mutability, use `&mut config` where saves are needed, but keep db read-only for now.

2. **Decouple Database Path**: Change `Database::new(path: PathBuf) -> Result<Self>` to accept the path explicitly, called in main with `Database::new(config::db_path()?)`. This reduces coupling to the config module.

### Implementation Steps

1. Update `Database::new` to accept `path: PathBuf` parameter and use it for `Connection::open(path)`.

2. In `main.rs`, after loading `config`, add `let db = Database::new(config::db_path()?)?;`.

3. Update dot module functions (e.g., `apply_all`, `fetch_modified`) to accept `&db: &Database` as an additional parameter, and remove internal `Database::new()` calls.

4. Pass `&db` to `Dotfile` methods and other sub-functions.

5. For config mutations (e.g., in `set_repo_active_subdirs`), continue using `&mut config`.

6. Add basic tests for single database usage.

This keeps changes minimal, improves efficiency and testability with simple parameter passing, suitable for the current app size.
