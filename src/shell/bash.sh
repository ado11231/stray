if [[ $- == *i* && -t 0 && -t 1 && -z ${SSH_CONNECTION-} ]] &&
   [[ -z ${TMUX-} || -z ${STRAY_TTY-} ]] &&
   [[ ${STRAY_TTY-} != "$(/usr/bin/tty 2>/dev/null)" ]]; then
    if shopt -q login_shell; then
        exec @STRAY_EXECUTABLE@ start --shell "$BASH" --login
    else
        exec @STRAY_EXECUTABLE@ start --shell "$BASH"
    fi
fi
