# bash completion for pj. Source it from ~/.bashrc, or copy it to
# ~/.local/share/bash-completion/completions/pj.
_pj() {
	local cur=${COMP_WORDS[COMP_CWORD]}
	# Project and session names may hold spaces, so words are split on newlines only.
	local IFS=$'\n'
	if ((COMP_CWORD == 1)); then
		local words
		words=$'save\nstop\n-\n'$(command pj --complete projects | cut -f1)
		COMPREPLY=($(compgen -W "$words" -- "$cur") $(compgen -d -- "$cur"))
		# Folders get a trailing / and names with spaces get quoted.
		compopt -o filenames 2>/dev/null
	elif ((COMP_CWORD == 2)) && [[ ${COMP_WORDS[1]} == save || ${COMP_WORDS[1]} == stop ]]; then
		COMPREPLY=($(compgen -W "$(command pj --complete sessions)" -- "$cur"))
	fi
}
complete -F _pj pj
