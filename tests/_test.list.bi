:i count 8
:b shell 70
./bin/torvo b tests/hello.torv -o tests/out/hello && ./tests/out/hello
:i returncode 6
:b stdout 42
Compiled program to tests/out/hello
Hello

:b stderr 0

:b shell 103
./bin/torvo b tests/func_declaration.torv -o tests/out/func_declaration && ./tests/out/func_declaration
:i returncode 6
:b stdout 53
Compiled program to tests/out/func_declaration
Hello

:b stderr 0

:b shell 94
./bin/torvo b tests/global_string.torv -o tests/out/global_string && ./tests/out/global_string
:i returncode 18
:b stdout 62
Compiled program to tests/out/global_string
Hello from global

:b stderr 0

:b shell 124
./bin/torvo b tests/global_string_from_func.torv -o tests/out/global_string_from_func && ./tests/out/global_string_from_func
:i returncode 18
:b stdout 72
Compiled program to tests/out/global_string_from_func
Hello from global

:b stderr 0

:b shell 61
./bin/torvo b tests/if.torv -o tests/out/if && ./tests/out/if
:i returncode 0
:b stdout 55
Compiled program to tests/out/if
it's true!
it's false

:b stderr 0

:b shell 91
./bin/torvo b tests/if_returning.torv -o tests/out/if_returning && ./tests/out/if_returning
:i returncode 0
:b stdout 65
Compiled program to tests/out/if_returning
it's true!
it's false

:b stderr 0

:b shell 88
./bin/torvo b tests/record_type.torv -o tests/out/record_type && ./tests/out/record_type
:i returncode 18
:b stdout 60
Compiled program to tests/out/record_type
Hello from record

:b stderr 0

:b shell 82
./bin/torvo b tests/recursion.torv -o tests/out/recursion && ./tests/out/recursion
:i returncode 0
:b stdout 47
Compiled program to tests/out/recursion
got 10

:b stderr 0

