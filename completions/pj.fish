# `command` reaches the binary even where a fish function named pj exists.
complete -c pj -f
complete -c pj -n __fish_is_first_arg -a save -d 'Save a session layout'
complete -c pj -n __fish_is_first_arg -a stop -d 'Save the layout and close the session'
complete -c pj -n __fish_is_first_arg -a - -d 'Previous project'
complete -c pj -n __fish_is_first_arg -a '(command pj --complete projects)'
complete -c pj -n __fish_is_first_arg -a '(__fish_complete_directories)'
complete -c pj -n '__fish_seen_subcommand_from save stop; and test (count (commandline -opc)) -eq 2' -a '(command pj --complete sessions)'
