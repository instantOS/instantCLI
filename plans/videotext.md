Planned Improvements to `ins video render`

I have an adjustment idea for displaying text in a video

The current behavior should be that if the following is encountered

```
`video segment`

---

some custom text

---

`video segment`
```


Then the video gets paused and the custom text gets shown for 5 seconds. 


I want to extend this behavior

```
`video segment`

---

some custom text

---

Some other custom text

---

Third custom text

---

`video segment`
```

In this case I have multiple blocks of custom text between the video segments. 
When rendering I want the editor to create a card for each of the custom text
blocks and show the cards in a sequence, each for 5 seconds.


