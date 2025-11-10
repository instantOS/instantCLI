
This uses some older utilities which might not be the best modern way to do
things, as an example, this is the best way to currently get the volume level. 

```
wpctl get-volume @DEFAULT_AUDIO_SINK@ | awk '{print $2*100}' | cut -d. -f1
```

Replace the outdated non-best-practice commands with modern ones (the wpctl is
also a lot less hacky and shorter than what was previously used)


