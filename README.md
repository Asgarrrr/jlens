# jlens

Ultra-performant terminal JSON viewer.

Open any JSON file, any size, instantly. Navigate, search, filter, diff, export — one tool.

## Features

**5 view modes** switchable with `1`-`5`:

- **Tree** — collapsible tree with syntax highlighting, vim navigation, search highlighting
- **Table** — auto-detected array-of-objects as sortable columns with zebra striping
- **Raw** — pretty-printed JSON with line numbers and syntax highlighting
- **Paths** — every leaf value with its full JSON path
- **Stats** — document overview, type distribution, depth analysis

**Search** (`/`) — incremental regex-capable search with match highlighting and navigation

**Filter** (`:`) — jq-like expression language:
```
.users                     # field access
.[0]                       # index
.items[]                   # iterate
.data | keys               # pipe to builtin
.data | length             # count elements
.values | flatten           # flatten arrays
```

**Zoom mode** (`z`/`Z`) — zoom into any container node. It becomes the root. All views reflect the subtree. Stack-based: zoom → zoom → pop → pop.

**Adaptive preview pane** (`p`) — toggle a bottom panel that auto-detects content:
- Array of numbers → sparkline chart with min/max/avg
- Array of objects → auto-table preview (first 10 rows)
- Array of strings → scrollable list
- String → detect URL, ISO date, base64, embedded JSON
- Object → key summary with types

Resize with `+`/`-`.

**Fuzzy path finder** (`@`) — telescope-style overlay. Type to fuzzy-search all paths in the document. Jump to any node instantly. Scores consecutive matches, word boundaries, and path separators.

**Structural diff** (`--diff`) — compare two JSON files side-by-side with added/removed/modified highlighting and full-row background tinting

**Export** (`Ctrl+S`) — export the selected subtree or full document to a file

**Lazy loading** — files over 500 MB are shallow-parsed via mmap. Expand sections on demand without loading the entire file into memory.

## Performance

| File size | Parse time |
|-----------|-----------|
| 1 KB | <1ms |
| 1 MB | 3ms |
| 10 MB | 3ms |
| 100 MB | 3ms |
| 1 GB | 2ms |

- 30fps rendering with dirty-flag optimization (zero CPU when idle)
- Zero-allocation syntax highlighting (borrowed slices, no per-frame heap allocs)
- Pre-computed theme styles (no `Style::new()` chains in render paths)
- Single-string RawView with byte offsets (no per-line heap allocations)
- Arena-based document model with append-only lazy expansion

## Install

```sh
cargo install --path .
```

Or build from source:

```sh
git clone https://github.com/Asgarrrr/jlens
cd jlens
cargo build --release
./target/release/jlens your_file.json
```

## Usage

```sh
jlens data.json                        # open file
jlens -                                # read from stdin
cat data.json | jlens                  # pipe
jlens a.json --diff b.json             # structural diff
```

## Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `j` / `Down` | Move down |
| `k` / `Up` | Move up |
| `Ctrl+D` / `PageDown` | Page down |
| `Ctrl+U` / `PageUp` | Page up |
| `Home` | Go to top |
| `G` / `End` | Go to bottom |

### Tree view

| Key | Action |
|-----|--------|
| `Enter` / `Space` | Toggle expand/collapse |
| `l` / `Right` | Expand node |
| `h` / `Left` | Collapse node |
| `e` | Expand all |
| `E` | Collapse all |

### Table view

| Key | Action |
|-----|--------|
| `Tab` | Next column |
| `Shift+Tab` | Previous column |
| `s` | Cycle sort |

### Global

| Key | Action |
|-----|--------|
| `1`-`5` | Switch view mode |
| `/` / `Ctrl+F` | Search |
| `n` / `N` | Next/previous match |
| `:` | Filter |
| `@` | Fuzzy path finder |
| `z` | Zoom into selected node |
| `Z` | Zoom out (pop) |
| `p` | Toggle preview pane |
| `+` / `-` | Resize preview pane |
| `y` | Copy value |
| `Y` | Copy path |
| `Ctrl+S` | Export |
| `?` | Help |
| `q` / `Ctrl+C` | Quit |

### Search bar

| Key | Action |
|-----|--------|
| `Ctrl+R` | Toggle regex mode |
| `Ctrl+N` / `Down` | Next match |
| `Ctrl+P` / `Up` | Previous match |
| `Enter` | Confirm and close |
| `Esc` | Cancel |

## Themes

Built-in Catppuccin Mocha (dark) and Latte (light) themes:

```sh
jlens --theme dark data.json
jlens --theme light data.json
```

## Architecture

```
src/
  app.rs              App struct, event loop, view dispatch
  app/
    terminal.rs       Terminal lifecycle (raw mode, panic recovery)
    diff.rs           Diff mode TUI
    search.rs         Search state machine + bar widget
    export.rs         Export state + bar widget
    filter.rs         Filter state + bar widget
  keymap.rs           Action enum + key-to-action mapping
  event.rs            Terminal event polling
  preview.rs          Adaptive preview pane (sparkline, table, string detection)
  finder.rs           Fuzzy path finder overlay
  config.rs           TOML config loading
  model/
    node.rs           Arena-based JSON document model
    lazy.rs           Mmap-backed lazy loading with shallow parse
  parser/
    detect.rs         Auto-detect parse strategy by file size
    full.rs           Full serde_json parse
    mmap.rs           Memory-mapped file parse
    scan.rs           Byte-level JSON scanner
    streaming.rs      Streaming parse support
  views/
    tree.rs           Collapsible tree view
    table.rs          Auto-detected table view
    raw.rs            Syntax-highlighted raw JSON
    path.rs           Leaf paths view
    stats.rs          Document statistics
  diff/
    algo.rs           Structural diff algorithm
    view.rs           Diff tree view
  filter/
    parse.rs          Expression parser
    eval.rs           Expression evaluator
  search.rs           Full-text + regex search
  theme.rs            Catppuccin color themes
  ui.rs               Layout, toolbar, status bar, help overlay
  util.rs             Scroll state, display width, formatting
```

## License

MIT
