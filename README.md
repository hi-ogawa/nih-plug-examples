# nih-plug examples

simple examples to get familiar with nih-plug and egui

```sh
# run as a standalone app (e.g. jack app)
cargo run -p midi_keyboard
cargo run -p soundfont_player -- --connect-jack-midi-input midi_keyboard:midi_output
```

![image](https://user-images.githubusercontent.com/4232207/208288590-4c653dde-1bcc-4d25-97a1-ca870dda6a1c.png)

![image](https://user-images.githubusercontent.com/4232207/210125843-849a2e17-5de6-4666-a785-e016ff05f4ea.png)

## references

- https://github.com/robbert-vdh/nih-plug/
- https://github.com/emilk/egui
- https://wiki.archlinux.org/title/PipeWire
