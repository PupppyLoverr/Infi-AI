# Solyra Terminal shell integration for fish.
function __solyra_mark --on-event fish_prompt
    printf '\e]133;D;%s\e\\' $status
    printf '\e]7;file://%s%s\e\\' (hostname) $PWD
end
if not set -q SOLYRA_KEEP_PROMPT; and not functions -q __solyra_user_prompt
    function fish_prompt
        printf '\e]133;A\e\\'(set_color -o green)'➜'(set_color normal)'  '(set_color -o cyan)(prompt_pwd)(set_color normal)' \e]133;B\e\\'
    end
end
