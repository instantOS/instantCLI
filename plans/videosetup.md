add an `ins video setup` command. This command should ask for the auphonic API
key using the menu utils input, verify the key is correct (look up how to do
this using the auphonic API), store the key in the config file, and predownload
the whisper uv stuff needed if possible. 

It should check if the whisper stuff is already downloaded before attempting it
again. Maybe uv can do that for us using uvx and --version something. 

If the key is already in the config, then it should just verify it. 

