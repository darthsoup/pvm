# PVM shell integration (fish). Reload this script after upgrading PVM.
# Save: pvm init --shell fish > ~/.config/fish/conf.d/pvm.fish
functions -e _pvm_auto_switch
function _pvm_auto_switch --on-variable PWD
  set -l token ''
  set -l enabled 1
  if test -r "$HOME/.pvm/hook-state"
    read -d \t token enabled < "$HOME/.pvm/hook-state"
  end
  if test "$enabled" = 0
    set -e PVM_PHP_VERSION
    set -e _PVM_CACHE
    return 0
  end
  set -l dir "$PWD"
  set -l wanted ''
  set -l source ''
  while true
    for f in .pvmrc .php-version .pvm
      if test -f "$dir/$f"
        set source "$dir/$f"
        # Default read scope updates the function variable. --local would
        # shadow it inside this for/if block and discard the result afterward.
        read -z wanted < "$source"
        break
      end
    end
    if test -n "$source"; or test "$dir" = /
      break
    end
    set dir (string replace -r '/[^/]*$' '' -- "$dir")
    if test -z "$dir"
      set dir /
    end
  end
  # Keep the original bytes for failure deduplication. A valid request is one
  # line; invalid multiline requests still reach Rust validation. Avoid a pipe
  # here: Fish forks pipeline stages even when both commands are builtins.
  set -l raw_wanted "$wanted"
  set wanted (string trim -- "$wanted")
  if test -z "$wanted"
    set -e PVM_PHP_VERSION
    set -e _PVM_CACHE
    set -e _PVM_FAILURE
    return 0
  end
  if test -n "$token"; and test "$_PVM_CACHE[1]" = "$token"; and test "$PVM_PHP_VERSION" = "$wanted"; and test "$_PVM_CACHE[2]" = "$source"
    return 0
  end
  command -v pvm >/dev/null 2>&1; or return 0
  set -l result
  set -l status_code
  if test "$_PVM_FAILURE[1]" = "$source"; and test "$_PVM_FAILURE[2]" = "$raw_wanted"; and test "$_PVM_FAILURE[3]" = "$token"
    pvm auto --hook 2>/dev/null | read -a -d \t result
    set status_code $pipestatus[1]
  else
    pvm auto --hook | read -a -d \t result
    set status_code $pipestatus[1]
  end
  if test "$status_code" = 0; and test -n "$result[1]"; and test -n "$result[2]"
    set -g _PVM_CACHE "$result[1]" "$source"
    set -g PVM_PHP_VERSION "$wanted"
    set -e _PVM_FAILURE
    if test "$result[3]" = 1
      printf 'Now using PHP %s (from %s)\n' "$result[2]" "$source" >&2
    end
  else
    set -e PVM_PHP_VERSION
    set -e _PVM_CACHE
    set -g _PVM_FAILURE "$source" "$raw_wanted" "$token"
  end
  return 0
end
set -e PVM_PHP_VERSION
set -e _PVM_CACHE
set -e _PVM_FAILURE
_pvm_auto_switch
