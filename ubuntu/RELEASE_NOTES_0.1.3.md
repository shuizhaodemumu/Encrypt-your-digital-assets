# Ubuntu 0.1.3 · Encrypt your digital assets

Ubuntu 0.1.3 makes the video control bar hide after one second of actual pointer inactivity. The controls slide into a narrow bottom dock and slide back out when the pointer moves. Pointer events are captured across the video surface and controls; unchanged motion events do not reset the idle timer. Video playback continues looping in the same player window.

The GTK regression now waits for the reveal animation to finish and asserts the actual hidden state. Formatting, 13 Rust tests, optimized test and production builds, and repeated Xvfb playback checks passed. The 0.1.3 DEB was installed on Ubuntu 26.04.1 x86_64, its installed package version was confirmed, and the application launched successfully. User acceptance passed.

This edition does not create a boot or login autostart entry. This release does not change the encryption or licensing format.
