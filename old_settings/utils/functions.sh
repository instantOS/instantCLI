#!/usr/bin/env bash

sidebar() {
    querystring="${1:-search...}"
    shift
    if [ -n "$SIDEBARPOS" ]; then
        case "$SIDEBARPOS" in
        center)
            instantmenu -r -it "$SIDEBARSEARCH" -ps "${SIDEBARINDEX:-1}" -l 2000 -w -400 -i -h -1 -c -bw 4 -H -q "$querystring" "$@" -pm
            SIDEBARSEARCH=
            return
            ;;
        left)
            SIDE_X="0"
            ;;
        right)
            SIDE_X="10000"
            ;;
        *)
            imenu -e 'invalid settings position'
            iconf -d settingsposition
            exit 1
            ;;
        esac
    else
        SIDE_X="10000"
    fi
    instantmenu -r -it "$SIDEBARSEARCH" -ps "${SIDEBARINDEX:-1}" -l 2000 -w -400 -i -h -1 -x "$SIDE_X" -y -1 -bw 4 -H -q "$querystring" "$@" -pm
    SIDEBARSEARCH=
}

die() {
    imenu -m "error: $1"
    exit 1
}

# System to attach data to functions see: https://github.com/erichs/composure

for f in about menu; do eval "$f() { :; }"; done
unset f # "register" keywords

meta() {
    typeset funcname="$1"
    typeset keyword="$2"

    if [ -z "$funcname" ] || [ -z "$keyword" ]; then
        printf '%s\n' 'missing parameter(s)'
        return
    fi

    typeset -f -- "$funcname" |
        sed -n "/$keyword / s/['\";]*$//;s/^[ 	]*$keyword ['\"]*\([^([].*\)*$/\1/p" ||
        echo "name: $funcname; kw: $keyword" 1>&2
}

_shell() {
    typeset this=$(ps -o comm -p $$ | tail -1 | awk '{print $NF}' | sed 's/^-*//')
    echo "${this##*/}"
}

list_func_names() {
    typeset shell="$(_shell)"
    if [ "$shell" = "bash" ] || [ "$shell" = "sh" ] || echo "$SHELL" | grep -q "bash$"; then
        typeset -F | awk '{print $3}'
    else
        typeset +f | sed 's/().*$//'
    fi
}

list_users() {
    # list real human users (uid above 1000, not 'nobody')  
    awk -F: '$3 >= 1000 && $1 != "nobody" {print $1}' /etc/passwd
}
