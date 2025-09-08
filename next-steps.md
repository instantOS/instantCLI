# Refactor

## Dotfile dir listing

Implement a way to get a list of all active dotfile dirs, in order of their relevance.
Repos are ordered by relevance in the config, dotfile dirs belonging to a repo are also ordered by relevance in the config.

Investigate all places where dotfile dirs are listed and refactor them to use the new function.

## Config handling
