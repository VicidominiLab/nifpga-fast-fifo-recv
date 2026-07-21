# nifpga-fast-fifo-recv
An unofficial Python module for fast reading of data from NI FPGA FIFOs with a separate thread.

This module provides in Python a class and the methods to start a separate thread which acquire continously 
data and store them into a queue which can be read asynchronously. 
This allows to receive data from FPGA in Python ideally without loosing data even for fast data-rate.

The module is developed in Rust by using `maturin`, `pyo3` and the Rust library `nifpga-dll`, an unofficial Rust bindings to `NiFpga.dll` library.
As described in the example below, `nifpga-fast-fifo-recv` does not require the `nifpga` Python module but in typical applications is strongly recommended to use with it.

Disclaimer: **The `nifpga-fast-fifo-recv` Python module is NOT an official NI library!** This library is an independent and autonomous development, unaffiliated with NI.









## Usage ##
### Installation ###
```
pip install nifpga-fast-fifo-recv
```

### Example ###

```python
from nifpga_fast_fifo_recv import NifpgaFastFifoRecv
fifo = NifpgaFastFifoRecv(bitfile = "myfile.lvbitx",
                          signature = bitfile_signature,
                          ni_address = "RIO0",
                          run = False,
                          close_on_reset = True,
                          fifo = fifo_number,
                          dma_buffer_size = 500000,
                          fifo_reading_buffer = 10000,
                          min_packet = 2,
                          )

fifo.thread_start()

assert(fifo.thread_status()==True)

for i in range(0,100):
    data=fifo.get_data()
    print(data)
    do_something_complicate(data)
    
fifo.thread_stop()

assert(fifo.thread_status()==False)
```

Please note that in the example above `run = False` that means that the FPGA suppose to be already programmed
with the identical bitfile and in a run state. This can be done by external program or a high-level LabView code,
or the python script before or in a separate thread/process. For example, it is possible to make a script load,
configure some register and run the bitfile by the standard `nifpga` library.
Once it is running it is possible at the same time use into another thread or process the
`NifpgaFastFifoRecv`, do the `thread_start()` and `get_data()`.

If you do not need to control registers or other things you can directly run the bitfile from `NifpgaFastFifoRecv`
simply by set `run = True`.

### How to get the `signature` and the `fifo_number` (from FIFO name)
The `nifpga_fast_fifo_recv` in can work alone but in real application is used with `nifpga`.
The bit file `.lvbitx` is actually an XML file. There is possible to find its signature and the correspondence between 
FIFO-number and FIFO name given at LabView compilation time.
Note typically if you have a single fifo in the bitfile, `fifo=0`.
The `nifpga` library can ease to extract the signature and the fifo number from the bitfile.

```python
import nifpga
bitfile_reader = nifpga.Bitfile(bitfile)
bitfile_signature = bitfile_reader.signature
bitfile_fifo_number = {i: n for n, i in enumerate(self.bitfile_reader.fifos.keys())}
fifo_number = bitfile_fifo_number["MYFIFO"]
```


### Object structure ###
The module structure is the following:

```
class NifpgaFastFifoRecv(object)
     |
     |  NifpgaFastFifoRecv(bitfile='X', signature='ABCD12345678', ni_address='RI0', run=False, close_on_reset=False, fifo=0, dma_buffer_size=50000, fifo_reading_buffer=10000, min_packet=1)
     |  
     |  Methods defined here:
     |  
     |  get_conf(self, /)
     |      get the current configuration
     |  
     |  get_data(self, /)
     |      get data from the internal queue
     |  
     |  thread_is_running(self, /)
     |      thread_is_running return true is thread is running
     |  
     |  thread_start(self, /)
     |      thread_start run the thread
     |  
     |  thread_stop(self, /)
     |      thread_stop stop the thread
```



## Developer ##

In order to compile this project you need *maturin* 

```
pip install maturin
```

and you need also the **NiFpga.dll** installed in your system.

### Build and publish Windows wheels locally

GitHub Actions cannot build this project because the proprietary NI libraries are
required. On a Windows machine with the NI FPGA runtime, Rust, MSYS2 UCRT64, and
the desired CPython versions installed, run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\release.ps1
```

The script adds `C:\msys64\ucrt64\bin` to `PATH`, discovers every CPython 3.10+
registered with the Windows `py` launcher, builds a release wheel for each one,
checks the wheel metadata, and tests each wheel in a clean virtual environment.
When an explicitly requested version is not installed system-wide, the script
downloads an isolated, uv-managed CPython into `.release-python`. This directory
is ignored by Git and can be deleted after the release. At least one system
Python 3.10+ installation is needed to bootstrap the release tools. Use
`-PythonInstallDir` to select a different temporary location.
The same workflow is available through the batch wrapper:

```bat
build-wheels.bat
```

To require a specific complete set of interpreters (and fail if one is missing),
use for example:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\release.ps1 -PythonVersion 3.10,3.11,3.12,3.13,3.14
```

Wheels are written to `dist`. Upload is intentionally separate from the default
build. Configure a PyPI API token in `$env:TWINE_PASSWORD` and use `__token__` as
the username:

```powershell
$env:TWINE_USERNAME = "__token__"
$env:TWINE_PASSWORD = "pypi-your-token"
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\release.ps1 -PythonVersion 3.10,3.11,3.12,3.13,3.14 -Publish
```

Use `-Repository testpypi -Publish` for a TestPyPI trial. Existing files in
`dist` are included in validation and upload, so use a fresh output directory
for each release (for example, `-OutDir dist-0.101.11`).

The publishing batch wrapper automatically supplies `-Publish` and forwards any
additional options:

```bat
publish-wheels.bat -PythonVersion 3.10,3.11,3.12,3.13,3.14
publish-wheels.bat -Repository testpypi -PythonVersion 3.13
```

# License #
Copyright (c) 2023 Istituto Italiano di Tecnologia

Permission is hereby granted, free of charge, to any person
obtaining a copy of this software and associated documentation
files (the "Software"), to deal in the Software without
restriction, including without limitation the rights to use,
copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the
Software is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES
OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
OTHER DEALINGS IN THE SOFTWARE.

