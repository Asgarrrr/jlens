# jlens

Ultra-performant terminal JSON viewer.

Open any JSON file, any size, instantly. Navigate, search, filter, diff, export — one tool.

## Features

**6 view modes** — switch with `1`-`6` or the view menu (`v`):

- **Tree** — collapsible tree with syntax highlighting, vim navigation, search highlighting
- **Table** — auto-detected array-of-objects as sortable columns with zebra striping
- **Raw** — pretty-printed JSON with line numbers and syntax highlighting
- **Paths** — every leaf value with its full JSON path
- **Stats** — document overview, type distribution, depth analysis
- **Schema** — inferred structure with types, presence percentages, and mixed-type detection

**Search** (`/`) — incremental regex-capable search with match highlighting and navigation

**Filter** (`:`) — jq-like expression language with live preview, autocomplete, and history:
```
.users[] | select(.age >= 30)       # filter by predicate
.items | map(.price * .qty)          # transform values
sort_by(.timestamp)                  # sort
.data | select(.name == "Alice")     # string comparison
(.a + .b) * 2                        # arithmetic
map(.category) | unique              # deduplicate
. | length                           # count
.[0] | keys                          # inspect structure
```

Supports: `select`, `map`, `sort_by`, comparisons (`==`, `!=`, `>`, `<`, `>=`, `<=`), boolean logic (`and`, `or`, `not`), arithmetic (`+`, `-`, `*`, `/`), and 16 builtins (`length`, `keys`, `values`, `type`, `flatten`, `first`, `last`, `reverse`, `unique`, `sort`, `min`, `max`, `not`, `to_number`, `to_string`, `ascii_downcase`).

**Zoom mode** (`z`/`Z`) — zoom into any container node. It becomes the root. All views reflect the subtree.

**Preview pane** (`p`) — toggle a side panel that auto-detects content:
- Array of numbers → sparkline chart
- Array of objects → table preview
- String → detect URL, ISO date, embedded JSON
- Object → key summary with types

**Fuzzy path finder** (`@`) — telescope-style overlay to jump to any node instantly.

**Structural diff** (`--diff`) — compare two JSON files with added/removed/modified highlighting.

**Export** (`Ctrl+S`) — export the selected subtree or full document to a file.

**Lazy loading** — all files use mmap-backed progressive parsing. No file size limit.

## Performance

| File size | Parse time |
|-----------|-----------|
| 1 KB | <1ms |
| 1 MB | 3ms |
| 10 MB | 3ms |
| 100 MB | 3ms |
| 1 GB | 2ms |

## Install

```sh
cargo install --path .
```

## Usage

```sh
jlens data.json                        # open file
jlens -                                # read from stdin
cat data.json | jlens                  # pipe
jlens a.json --diff b.json             # structural diff
jlens --init                            # generate config file
```

## Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `j` / `Down` | Move down |
| `k` / `Up` | Move up |
| `Ctrl+D` / `PageDown` | Page down |
| `Ctrl+U` / `PageUp` | Page up |
| `Home` / `Ctrl+A` | Go to top |
| `G` / `End` | Go to bottom |

### Views

| Key | Action |
|-----|--------|
| `1`-`6` | Switch view (Tree, Table, Raw, Paths, Stats, Schema) |
| `v` | View selector menu |
| `p` | Toggle preview pane |
| `+` / `-` | Resize preview |

### Tree

| Key | Action |
|-----|--------|
| `Enter` / `Space` | Toggle expand/collapse |
| `l` / `Right` | Expand node |
| `h` / `Left` | Collapse node |
| `e` | Expand all |
| `E` | Collapse all |
| `z` | Zoom in |
| `Z` | Zoom out |

### Filter (`:`)

| Key | Action |
|-----|--------|
| `Tab` | Switch focus (tree / filter) |
| `Enter` | Apply filter |
| `Esc` | Close filter |
| `Up` / `Down` | History / suggestions |
| `Left` / `Right` | Move cursor |
| `Ctrl+W` | Delete word |
| `Ctrl+U` | Clear line |

### Global

| Key | Action |
|-----|--------|
| `/` / `Ctrl+F` | Search |
| `:` | Filter |
| `@` | Fuzzy path finder |
| `y` | Copy value |
| `Y` | Copy path |
| `Ctrl+S` | Export |
| `?` | Help |
| `q` / `Ctrl+C` | Quit |

## Configuration

Generate a default config:

```sh
jlens --init
```

Config location: `~/.config/jlens/config.toml` (Linux) or `~/Library/Application Support/jlens/config.toml` (macOS).

```toml
[general]
default_view = "tree"
tick_rate_ms = 33

[search]
regex = false

[keybindings]
# quit = "q"
# search = "/"

[theme]
base = "dark"

[theme.overrides]
# bg = "#1e1e2e"
# fg = "#cdd6f4"
```

## Themes

Built-in Catppuccin Mocha (dark) and Latte (light):

```sh
jlens --theme dark data.json
jlens --theme light data.json
```

## License

MIT
