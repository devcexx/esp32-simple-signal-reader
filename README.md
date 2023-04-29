# ESP32 Simple Signal Reader

This project allows to stream digital signal coming from a GPIO port
of an ESP32 through serial, and store and visualize it in the
computer. For me, it has been helpful as an incredibly cheap 433 MHz
receiver for doing development. The most I was able to achieve is
sending a signal sampled at a frequency of 100 KHz using a serial port
with 128k baud rate.

It consists on two pieces of software:
 - ESP32 program: The program based on
   [esp-idf](https://github.com/espressif/esp-idf) that is flashed
   into the ESP32 for collecting and sending the data to the
   computer. By default, it uses the main USB serial port of ESP32 for
   logs and diagnostics, while the raw signal data is sent through the
   secondary serial ports, that can be read from the computer using a
   USB-to-UART module.
   
 - esp32-samples-reader: A Rust program (Could have been done in
   Python, but where's the fun in that) that allows you to read the
   samples and 1) store it in a Wave file or 2) stream it as PCM audio
   to Pulseaudio so it can be recorded in realtime using tools like
   Audacity.

## Uploading to ESP32
 - Edit the definitions of the `main/signalreader.c` file for changing
   the configuration of the program (GPIO ports, baud rate, sample
   rate, etc).
 - Check [esp-idf](https://github.com/espressif/esp-idf) documentation
   for more information about how to install the development tools
   from Espressif for being able to build and flash the program.
 - Flash the program using `idf.py -p /dev/tty<device> flash monitor`.
 
## Reading samples

Once the program is running in the ESP32 and your computer is
connected to the external ESP32 serial port through a USB-to-UART
module, you can run the following command to start recording the
signal into a wave file:

```bash
cargo run --release -- read-wav --port /dev/tty<UART-device> --sampling-rate X --baud-rate Y --output output.wav
```

Make sure the `--baud-rate` and the `--sampling-rate` parameters are
in sync with the ones configured in the ESP32. Otherwise the program
won't be to read properly data from the ESP32 and keep it in sync with
the time in the wave file.

The application also integrates with PulseAudio so signal data can be
continously sent to PulseAudio that can be recorded by normal
applications, like Audacity. For that, the application will create a
temporal null sink while it is running and will stream data to
it. While the application is running, other applications will be able
to read this data from the monitor of that sink. The application will
destroy the sink once it terminates.

The command is the following:
```bash
cargo run --release -- pulse-stream --port /dev/tty<UART-device> --sampling-rate X --baud-rate Y --output output.wav
```
