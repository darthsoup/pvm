# PVM shell integration (bash). Reload this script after upgrading PVM.
# Add: eval "$(pvm init --shell bash)" to ~/.bashrc
_pvm_auto_switch() {
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
        break
      fi
    done
    [[ -n "$source" || "$dir" == / ]] && break
    dir="${dir%/*}"
    [[ -n "$dir" ]] || dir=/
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
    PVM_PHP_VERSION="$wanted"
    _PVM_SOURCE="$source"
    unset _PVM_FAILURE_SOURCE _PVM_FAILURE_WANTED _PVM_FAILURE_TOKEN
    if [[ "$changed" == 1 ]]; then printf 'Now using PHP %s (from %s)\n' "$_PVM_RESOLVED" "$source" >&2; fi
  else
    unset PVM_PHP_VERSION _PVM_SOURCE _PVM_TOKEN
    _PVM_FAILURE_SOURCE="$source" _PVM_FAILURE_WANTED="$wanted" _PVM_FAILURE_TOKEN="$token"
  fi
  return 0
}
unset PVM_PHP_VERSION _PVM_SOURCE _PVM_TOKEN _PVM_FAILURE_SOURCE _PVM_FAILURE_WANTED _PVM_FAILURE_TOKEN
# Preserve both scalar and array PROMPT_COMMAND values and avoid duplicate hooks.
if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == 'declare -a'* ]]; then
  _pvm_present=0
  for _pvm_entry in "${PROMPT_COMMAND[@]}"; do [[ "$_pvm_entry" == _pvm_auto_switch ]] && _pvm_present=1; done
  if [[ "$_pvm_present" == 0 ]]; then PROMPT_COMMAND=(_pvm_auto_switch "${PROMPT_COMMAND[@]}"); fi
  unset _pvm_present _pvm_entry
elif [[ ";${PROMPT_COMMAND-};" != *';_pvm_auto_switch;'* ]]; then
  PROMPT_COMMAND="_pvm_auto_switch${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
fi
_pvm_auto_switch
