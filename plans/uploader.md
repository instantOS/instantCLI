
use something equivalent to this to screenshot to imgur

```sh
grim -g "$(slurp)" - | \
curl -s -F "image=@-" https://api.imgur.com/3/image \
     -H "Authorization: Client-ID 546c25a59c58ad7" | \
jq -r .data.link | \
wl-copy && notify-send "Imgur link copied"
```
