# Run known AI agent CLIs under `solyra-agent wrap` in interactive shells, so
# the Solyra notch can show their status and send them text. The program runs
# exactly as before (it is on a real terminal); only a status report is added.
#
# Turn it off: add `wrap=0` to ~/.config/solyra/agent.conf (Settings -> Agents),
# or `export SOLYRA_AGENT_WRAP=0`. Bypass once: `command claude ...`.
if [ -n "${PS1-}" ] && command -v solyra-agent >/dev/null 2>&1 \
   && [ -z "${SOLYRA_AGENT_WRAPPED-}" ] && [ "${SOLYRA_AGENT_WRAP-1}" != 0 ] \
   && ! grep -qx 'wrap=0' "${XDG_CONFIG_HOME:-$HOME/.config}/solyra/agent.conf" 2>/dev/null; then
  for _solyra_cli in claude codex gemini opencode aider; do
    eval "$_solyra_cli() {
      _solyra_bin=\$(type -P $_solyra_cli) || { echo \"$_solyra_cli: command not found\" >&2; return 127; }
      if [ -t 0 ] && [ -t 1 ]; then
        solyra-agent wrap --name $_solyra_cli -- \"\$_solyra_bin\" \"\$@\"
      else
        \"\$_solyra_bin\" \"\$@\"
      fi
    }"
  done
  unset _solyra_cli
fi
