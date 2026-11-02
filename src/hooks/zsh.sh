# PVM shell integration (zsh). Reload this script after upgrading PVM.
# Add: eval "$(pvm init --shell zsh)" to ~/.zshrc
_pvm_auto_switch() {
  emulate -L zsh
  local token="" enabled=1 dir="$PWD" wanted="" source="" f result_token result_version changed status_code
  if [[ -r "$HOME/.pvm/hook-state" ]]; then
    IFS=$'\t' read -r token enabled < "$HOME/.pvm/hook-state" || :
  fi
  if [[ "$enabled" == 0 ]]; then
    unset PVM_PHP_VERSION _PVM_SOURCE _PVM_TOKEN
    return 0
  fi
  while true; do
    for f in .pvmrc .php-version .pvm; do
      if [[ -f "$dir/$f" ]]; then
        source="$dir/$f"
        IFS= read -r -d '' wanted < "$source" || :
        break 2
      fi
    done
    [[ "$dir" == / ]] && break
    dir="${dir:h}"
  done
  wanted="${wanted#"${wanted%%[![:space:]]*}"}"
  wanted="${wanted%"${wanted##*[![:space:]]}"}"
  if [[ -z "$wanted" ]]; then
    unset PVM_PHP_VERSION _PVM_SOURCE _PVM_TOKEN _PVM_FAILURE_SOURCE _PVM_FAILURE_WANTED _PVM_FAILURE_TOKEN
    return 0
  fi
  if [[ -n "$token" && "${PVM_PHP_VERSION-}" == "$wanted" && "${_PVM_SOURCE-}" == "$source" && "${_PVM_TOKEN-}" == "$token" ]]; then
    return 0
  fi
  command -v pvm >/dev/null 2>&1 || return 0
  if [[ "${_PVM_FAILURE_SOURCE-}" == "$source" && "${_PVM_FAILURE_WANTED-}" == "$wanted" && "${_PVM_FAILURE_TOKEN-}" == "$token" ]]; then
    if IFS=$'\t' read -r result_token result_version changed < <(exec pvm auto --hook 2>/dev/null); then status_code=0; else status_code=1; fi
  else
    if IFS=$'\t' read -r result_token result_version changed < <(exec pvm auto --hook); then status_code=0; else status_code=1; fi
  fi
  # Only a committed activation emits this complete success record. Reading it
  # directly avoids a capture subshell and a second parse of the same line.
  if [[ "$status_code" == 0 && -n "$result_token" && -n "$result_version" && ( "$changed" == 0 || "$changed" == 1 ) ]]; then
    _PVM_TOKEN="$result_token" _PVM_RESOLVED="$result_version"
    typeset -g PVM_PHP_VERSION="$wanted" _PVM_SOURCE="$source" _PVM_TOKEN="$_PVM_TOKEN"
    unset _PVM_FAILURE_SOURCE _PVM_FAILURE_WANTED _PVM_FAILURE_TOKEN
    if [[ "$changed" == 1 ]]; then printf 'Now using PHP %s (from %s)\n' "$_PVM_RESOLVED" "$source" >&2; fi
  else
    unset PVM_PHP_VERSION _PVM_SOURCE _PVM_TOKEN
    typeset -g _PVM_FAILURE_SOURCE="$source" _PVM_FAILURE_WANTED="$wanted" _PVM_FAILURE_TOKEN="$token"
  fi
  return 0
}
unset PVM_PHP_VERSION _PVM_SOURCE _PVM_TOKEN _PVM_FAILURE_SOURCE _PVM_FAILURE_WANTED _PVM_FAILURE_TOKEN
# Re-register safely when the integration is sourced more than once.
autoload -U add-zsh-hook
add-zsh-hook -d chpwd _pvm_auto_switch
add-zsh-hook chpwd _pvm_auto_switch
_pvm_auto_switch
