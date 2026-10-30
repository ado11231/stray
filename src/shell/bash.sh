if [[ $- == *i* && -t 0 && -t 1 && -z ${SSH_CONNECTION-} ]] &&
   [[ -z ${TMUX-} || -z ${STRAY_TTY-} ]] &&
   [[ ${STRAY_TTY-} != "$(/usr/bin/tty 2>/dev/null)" ]]; then
    if shopt -q login_shell; then
        exec @STRAY_EXECUTABLE@ start --shell "$BASH" --login
    else
        exec @STRAY_EXECUTABLE@ start --shell "$BASH"
    fi
fi

# Inside Stray's own shell, mark each prompt with the last command's exit status, so the
# cat can flinch when a command fails. An empty line adds nothing to the history, so it
# does not count as a command.
if [[ $- == *i* && ${STRAY_TTY-} == "$(/usr/bin/tty 2>/dev/null)" ]]; then
    _stray_mark() {
        local stray_status=$? stray_latest
        stray_latest=$(HISTTIMEFORMAT= history 1)
        # The first prompt only notes the history, which is loaded after this file runs.
        if [[ -n ${_stray_started-} && $stray_latest != "$_stray_last" ]]; then
            printf '\033]@STRAY_PROMPT_MARK@;%s\007' "$stray_status"
        fi
        _stray_started=1
        _stray_last=$stray_latest
        return $stray_status
    }
    PROMPT_COMMAND="_stray_mark${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
fi
