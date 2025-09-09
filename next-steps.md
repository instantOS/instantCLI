# `add` Command Refactor

## Current Implementation

The current `instant dot add <path>` command automatically adds the specified dotfile to the highest-priority repository in the user's configuration. It does not ask for user input and is not dependent on the current working directory.

### Issues Identified

1.  **Makes Assumptions**: The command assumes the user always wants to add a new dotfile to their highest-priority (i.e., the last) repository. This may not always be the case, especially for users who organize their dotfiles across multiple repos (e.g., `work`, `personal`, `themes`).
2.  **No User Control**: The user has no way to specify which repository or which `dots_dir` within that repository the file should be added to. This can lead to dotfiles being stored in a disorganized or incorrect manner.
3.  **Lack of Interactivity**: The command succeeds or fails without any interactive prompts, which is not ideal for an operation that modifies the structure of a dotfile repository.

## Proposed Improvements

The goal is to make the `add` command intelligent, interactive, and intuitive, giving the user full control over where their dotfiles are stored.

### 1. CWD-Independent Logic

The command will take a single argument, `<path>`, which is the path to the dotfile in the home directory to be added. The command's behavior will not be affected by the current working directory.

### 2. Interactive Selection Process

When ambiguity exists, the tool will prompt the user to resolve it.

-   **Repository Selection**: If the user has multiple repositories configured, the command will present a list of them and ask the user to choose the destination repository.
-   **Dotfile Directory (`dots_dir`) Selection**: If the chosen repository has multiple `dots_dirs` defined in its `instantdots.toml` (e.g., `dots`, `themes`, `configs`), the command will prompt the user to select the target directory within the repo.

### 3. Clear Feedback

The command will provide explicit feedback about the action it has taken, confirming which file was added to which repository and `dots_dir`.

## Implementation Steps

1.  **Update `DotCommands::Add` in `src/main.rs`**:
    -   Ensure the command struct is simple, taking only the `path` argument.

2.  **Refactor `add_dotfile` in `src/dot/mod.rs`**:
    -   The function will receive the `config`, `db`, and `path`.
    -   **Repository Selection Logic**:
        -   If `config.repos.len() == 1`, select the only repository automatically.
        -   If `config.repos.len() > 1`, use the `dialoguer` crate to display a `Select` prompt listing the names of the available repositories.
    -   **`dots_dir` Selection Logic**:
        -   Once a repository is chosen, inspect its `dots_dirs` from its metadata.
        -   If the repo has only one `dots_dir`, select it automatically.
        -   If it has multiple, use a `dialoguer` `Select` prompt to ask the user which `dots_dir` to use.
    -   **File Operation**:
        -   Construct the final destination path inside the chosen repository and `dots_dir`.
        -   Create any necessary parent directories within the repository (e.g., `.../dots/.config/`).
        -   Copy the file from the home directory (`~/...`) to the destination.
    -   **Database Update**:
        -   Create a new `Dotfile` struct for the added file.
        -   Call `get_source_hash()` on the new dotfile to compute its hash and add it to the database as an unmodified, tracked file.
    -   **Final Feedback**:
        -   Print a confirmation message, e.g., `Added ~/.bashrc to repo 'my-dots' in directory 'dots'`. 

This approach makes the `add` command much more powerful and aligns with the principle of keeping the user in control.
