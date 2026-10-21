if [[ $- == *i* && -t 0 && -t 1 && -z ${SSH_CONNECTION-} ]] &&
   [[ -z ${TMUX-} || -z ${STRAY_TTY-} ]] &&
   [[ ${STRAY_TTY-} != "$(/usr/bin/tty 2>/dev/null)" ]]; then
    if [[ -o login ]]; then
        exec @STRAY_EXECUTABLE@ start --shell "${commands[zsh]:-/bin/zsh}" --login
    else
        exec @STRAY_EXECUTABLE@ start --shell "${commands[zsh]:-/bin/zsh}"
    fi
fi
