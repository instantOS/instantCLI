
I want to introduce a few optional questions to `ins arch install`. 

There will be quite a few of them, so the system to make a question optional and
changes to traits or other components should be well planned
out and provide good developer experience. In the final summary after all
mandatory questions (which already has cancel, review options), there should be
another option "advanced options" which leads to a list of the optional
questions. 

The first optional question is which kernel to use. Allow choosing between
linux, linux-lts, linux-zen. Default is linux. 

Keep in mind that optional questions might also have conditions for activation
just like the other questions, and that answering an optional question might
make another question mandatory. There already is a good dependency system in
place. 


