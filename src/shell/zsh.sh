if [[ $- == *i* && -t 0 && -t 1 && -z ${SSH_CONNECTION-} ]] &&
   [[ -z ${TMUX-} || -z ${STRAY_TTY-} ]] &&
   [[ ${STRAY_TTY-} != "$(/usr/bin/tty 2>/dev/null)" ]]; then
    if [[ -o login ]]; then
        exec @STRAY_EXECUTABLE@ start --shell "${commands[zsh]:-/bin/zsh}" --login
    else
        exec @STRAY_EXECUTABLE@ start --shell "${commands[zsh]:-/bin/zsh}"
    fi
fi

# Inside Stray's own shell, mark each prompt with the last command's exit status, so the
# cat can flinch when a command fails. An empty line does not count as a command.
if [[ $- == *i* && ${STRAY_TTY-} == "$(/usr/bin/tty 2>/dev/null)" ]]; then
    _stray_ran=
    _stray_preexec() { _stray_ran=1; }
    _stray_precmd() {
        local stray_status=$?
        [[ -n $_stray_ran ]] && printf '\033]@STRAY_PROMPT_MARK@;%s\007' "$stray_status"
        _stray_ran=
        return $stray_status
    }
    # First in line, so other prompt hooks cannot change $? before it is read.
    preexec_functions=(_stray_preexec $preexec_functions)
    precmd_functions=(_stray_precmd $precmd_functions)
fi
