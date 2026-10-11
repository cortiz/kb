//! Shell script templates and init code generators.

pub const KB_STARTUP_BLOCK_BEGIN: &str = "# >>> kb >>>";
pub const KB_STARTUP_BLOCK_END: &str = "# <<< kb <<<";

use super::dialects::ShellDialect;

pub fn generate_shell_init(
    dialect: ShellDialect,
    widget_key: &str,
    save_key: &str,
) -> String {
    let w_key = if widget_key.is_empty() { r"\ek" } else { widget_key };
    let s_key = if save_key.is_empty() { r"\eK" } else { save_key };

    match dialect {
        ShellDialect::Zsh => format!(
r#"# kb shell integration for zsh
_kb_widget() {{
  local out ret
  out=$(kb pick --widget --shell zsh --buffer "$BUFFER" --cursor "$CURSOR" 2>/dev/null)
  ret=$?
  if [[ $ret -eq 0 ]]; then
    local cmd cur
    if command -v jq >/dev/null 2>&1; then
      cmd=$(echo "$out" | jq -r .command)
      cur=$(echo "$out" | jq -r .cursor)
    elif command -v python3 >/dev/null 2>&1; then
      cmd=$(python3 -c "import sys, json; print(json.loads(sys.argv[1]).get('command', ''), end='')" "$out")
      cur=$(python3 -c "import sys, json; print(json.loads(sys.argv[1]).get('cursor', 0), end='')" "$out")
    fi
    BUFFER="$cmd"
    CURSOR="$cur"
    zle reset-prompt
  elif [[ $ret -eq 1 ]]; then
    local msg
    if command -v jq >/dev/null 2>&1; then
      msg=$(echo "$out" | jq -r .message)
    elif command -v python3 >/dev/null 2>&1; then
      msg=$(python3 -c "import sys, json; print(json.loads(sys.argv[1]).get('message', ''), end='')" "$out")
    fi
    [[ -n "$msg" ]] && zle -M "$msg"
  fi
}}
zle -N _kb_widget

_kb_save_last() {{
  local last_cmd
  last_cmd=$(fc -ln -1 2>/dev/null | sed 's/^[[:space:]]*//')
  if [[ -n "$last_cmd" ]]; then
    printf "%s\n" "$last_cmd" | kb add --type cmd --stdin
  fi
}}
zle -N _kb_save_last
alias kb-save-last=_kb_save_last

# Keybindings
local _kb_key="${{KB_WIDGET_KEY:-{w_key}}}"
local _kb_save="${{KB_SAVE_KEY:-{s_key}}}"
if [[ "$KB_WIDGET_FORCE" == "1" ]] || ! bindkey "$_kb_key" >/dev/null 2>&1 || [[ "$(bindkey "$_kb_key")" =~ "undefined-key" ]]; then
  bindkey "$_kb_key" _kb_widget
  bindkey -M vicmd "$_kb_key" _kb_widget 2>/dev/null || true
  bindkey -M viins "$_kb_key" _kb_widget 2>/dev/null || true
fi
if [[ "$KB_WIDGET_FORCE" == "1" ]] || ! bindkey "$_kb_save" >/dev/null 2>&1 || [[ "$(bindkey "$_kb_save")" =~ "undefined-key" ]]; then
  bindkey "$_kb_save" _kb_save_last
  bindkey -M vicmd "$_kb_save" _kb_save_last 2>/dev/null || true
  bindkey -M viins "$_kb_save" _kb_save_last 2>/dev/null || true
fi

# Completions
if (( $+commands[kb] )); then
  source <(kb completions zsh)
fi
"#
        ),
        ShellDialect::Bash => format!(
r#"# kb shell integration for bash
if [[ "${{BASH_VERSINFO[0]:-0}}" -lt 4 ]]; then
  _kb_widget() {{
    local out ret
    out=$(kb pick --widget --shell bash --buffer "$READLINE_LINE" --cursor "$READLINE_POINT" 2>/dev/null)
    ret=$?
    if [[ $ret -eq 0 ]]; then
      local cmd
      if command -v jq >/dev/null 2>&1; then
        cmd=$(echo "$out" | jq -r .command)
      elif command -v python3 >/dev/null 2>&1; then
        cmd=$(python3 -c "import sys, json; print(json.loads(sys.argv[1]).get('command', ''), end='')" "$out")
      fi
      if [[ -n "$cmd" ]]; then
        history -s "$cmd"
        printf "\nkb: command added to history (press Up arrow to recall; upgrade to bash 4+ for direct insertion)\n"
      fi
    fi
  }}
else
  _kb_widget() {{
    local out ret
    out=$(kb pick --widget --shell bash --buffer "$READLINE_LINE" --cursor "$READLINE_POINT" 2>/dev/null)
    ret=$?
    if [[ $ret -eq 0 ]]; then
      local cmd cur
      if command -v jq >/dev/null 2>&1; then
        cmd=$(echo "$out" | jq -r .command)
        cur=$(echo "$out" | jq -r .cursor)
      elif command -v python3 >/dev/null 2>&1; then
        cmd=$(python3 -c "import sys, json; print(json.loads(sys.argv[1]).get('command', ''), end='')" "$out")
        cur=$(python3 -c "import sys, json; print(json.loads(sys.argv[1]).get('cursor', 0), end='')" "$out")
      fi
      READLINE_LINE="$cmd"
      READLINE_POINT="$cur"
    elif [[ $ret -eq 1 ]]; then
      local msg
      if command -v jq >/dev/null 2>&1; then
        msg=$(echo "$out" | jq -r .message)
      elif command -v python3 >/dev/null 2>&1; then
        msg=$(python3 -c "import sys, json; print(json.loads(sys.argv[1]).get('message', ''), end='')" "$out")
      fi
      [[ -n "$msg" ]] && printf "\n%s\n" "$msg"
    fi
  }}
fi

_kb_save_last() {{
  local last_cmd
  last_cmd=$(fc -ln -1 2>/dev/null | sed 's/^[[:space:]]*//')
  if [[ -n "$last_cmd" ]]; then
    printf "%s\n" "$last_cmd" | kb add --type cmd --stdin
  fi
}}
alias kb-save-last=_kb_save_last

# Keybindings
_kb_key="${{KB_WIDGET_KEY:-{w_key}}}"
_kb_save="${{KB_SAVE_KEY:-{s_key}}}"
if [[ "$KB_WIDGET_FORCE" == "1" ]] || ! bind -p 2>/dev/null | grep -q "\"$_kb_key\":"; then
  bind -x "\"$_kb_key\": _kb_widget"
fi
if [[ "$KB_WIDGET_FORCE" == "1" ]] || ! bind -p 2>/dev/null | grep -q "\"$_kb_save\":"; then
  bind -x "\"$_kb_save\": _kb_save_last"
fi

# Completions
if command -v kb >/dev/null 2>&1; then
  source <(kb completions bash)
fi
"#
        ),
        ShellDialect::Fish => format!(
r#"# kb shell integration for fish
function _kb_widget
    set -l buf (commandline)
    set -l cur (commandline -C)
    set -l out (kb pick --widget --shell fish --buffer "$buf" --cursor "$cur" 2>/dev/null)
    set -l ret $status
    if test $ret -eq 0
        set -l cmd
        set -l new_cur
        if type -q jq
            set cmd (echo "$out" | jq -r .command)
            set new_cur (echo "$out" | jq -r .cursor)
        else if type -q python3
            set cmd (python3 -c "import sys, json; print(json.loads(sys.argv[1]).get('command', ''), end='')" "$out")
            set new_cur (python3 -c "import sys, json; print(json.loads(sys.argv[1]).get('cursor', 0), end='')" "$out")
        end
        commandline -r "$cmd"
        commandline -C "$new_cur"
    else if test $ret -eq 1
        set -l msg
        if type -q jq
            set msg (echo "$out" | jq -r .message)
        else if type -q python3
            set msg (python3 -c "import sys, json; print(json.loads(sys.argv[1]).get('message', ''), end='')" "$out")
        end
        if test -n "$msg"
            echo ""
            echo "$msg"
        end
    end
    commandline -f repaint
end

function _kb_save_last
    set -l last_cmd (history --max=1)
    if test -n "$last_cmd"
        printf "%s\n" "$last_cmd" | kb add --type cmd --stdin
    end
end
alias kb-save-last=_kb_save_last

# Keybindings
set -l _kb_key (set -q KB_WIDGET_KEY; and echo $KB_WIDGET_KEY; or echo "{w_key}")
set -l _kb_save (set -q KB_SAVE_KEY; and echo $KB_SAVE_KEY; or echo "{s_key}")
bind $_kb_key _kb_widget
bind -M insert $_kb_key _kb_widget 2>/dev/null
bind $_kb_save _kb_save_last
bind -M insert $_kb_save _kb_save_last 2>/dev/null

# Completions
if type -q kb
    kb completions fish | source
end
"#
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zsh_init_generation() {
        let script = generate_shell_init(ShellDialect::Zsh, r"\ek", r"\eK");
        assert!(script.contains("_kb_widget"), "must define widget function");
        assert!(script.contains("BUFFER="), "must set BUFFER");
        assert!(script.contains("CURSOR="), "must set CURSOR");
        assert!(script.contains("zle reset-prompt"), "must reset prompt");
        assert!(script.contains("zle -N _kb_widget"), "must register ZLE widget");
        assert!(script.contains("bindkey"), "must bind keys");
        assert!(script.contains("fc -ln -1"), "must read history for save-last");
        assert!(script.contains("kb add --type cmd --stdin"), "must pipe to kb add --type cmd --stdin");
        assert!(script.contains("kb completions zsh"), "must source completions");
    }

    #[test]
    fn test_bash_init_generation() {
        let script = generate_shell_init(ShellDialect::Bash, r"\ek", r"\eK");
        assert!(script.contains("BASH_VERSINFO"), "must check bash version");
        assert!(script.contains("READLINE_LINE="), "must set READLINE_LINE for bash 4+");
        assert!(script.contains("READLINE_POINT="), "must set READLINE_POINT for bash 4+");
        assert!(script.contains("history -s"), "must push to history in bash 3.2 fallback");
        assert!(script.contains("bind -x"), "must use bind -x");
        assert!(script.contains("fc -ln -1"), "must read history for save-last");
        assert!(script.contains("kb add --type cmd --stdin"), "must pipe to kb add --type cmd --stdin");
        assert!(script.contains("kb completions bash"), "must source completions");
    }

    #[test]
    fn test_fish_init_generation() {
        let script = generate_shell_init(ShellDialect::Fish, r"\ek", r"\eK");
        assert!(script.contains("function _kb_widget"), "must define fish function");
        assert!(script.contains("commandline -r"), "must replace commandline");
        assert!(script.contains("commandline -C"), "must set cursor");
        assert!(script.contains("commandline -f repaint"), "must repaint prompt");
        assert!(script.contains("history --max=1"), "must read history for save-last");
        assert!(script.contains("kb add --type cmd --stdin"), "must pipe to kb add --type cmd --stdin");
        assert!(script.contains("bind"), "must bind keys");
        assert!(script.contains("kb completions fish"), "must source completions");
    }

    #[test]
    fn test_custom_keys_in_init_generation() {
        let script = generate_shell_init(ShellDialect::Zsh, r"\ej", r"\eJ");
        assert!(script.contains(r"\ej"), "must use custom widget key");
        assert!(script.contains(r"\eJ"), "must use custom save key");
    }
}

