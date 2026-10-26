# Task 42 test app: acMain fails with an error raised by C code (its value a str; an exception
# object raised by Python code would crash acs.exe in PythonInterface::printErr).
import ac


def acMain(ac_version):
    ac.log("py_raise acMain")
    return int("not a number")


def acUpdate(deltaT):
    ac.log("never called")
