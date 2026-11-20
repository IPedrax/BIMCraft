"""Compile the local bridge against the editor installation on Apple Silicon."""
from pathlib import Path
import subprocess
import sys

engine = Path(sys.argv[1])
source = engine / "Source"
here = Path(__file__).resolve().parent
binary = engine / "Binaries/Mac"
include = ["Runtime/Core/Public", "Runtime/CoreUObject/Public", "Runtime/TraceLog/Public",
           "Runtime/AutoRTFM/Public", "Runtime/Projects/Public", "Runtime/Json/Public", "Runtime/Datasmith/DirectLink/Public",
           "Runtime/Datasmith/DatasmithCore/Public", "Developer/Datasmith/DatasmithExporter/Public"]
defines = """
WITH_SERVER_CODE=1 UE_ENABLE_INCLUDE_ORDER_DEPRECATED_IN_5_2=0 PLATFORM_MAC=1 PLATFORM_APPLE=1 PLATFORM_UNIX=1 PLATFORM_CPU_ARM_FAMILY=1 PLATFORM_64BITS=1
UBT_COMPILED_PLATFORM=Mac UE_BUILD_DEVELOPMENT=1 WITH_EDITOR=1 WITH_EDITORONLY_DATA=1
WITH_ENGINE=1 WITH_UNREAL_DEVELOPER_TOOLS=1 WITH_PLUGIN_SUPPORT=1 IS_MONOLITHIC=0 IS_PROGRAM=0
CORE_API= COREUOBJECT_API= TRACELOG_API= DATASMITHCORE_API= DATASMITHEXPORTER_API= DIRECTLINK_API= JSON_API=
UNICODE=1 _UNICODE=1 WITH_APPLICATION_CORE=1 UE_BUILD_MINIMAL=0
""".split()
args = ["clang++", "-x", "objective-c++", "-std=c++20", "-arch", "arm64", "-dynamiclib", "-fno-rtti", "-fno-exceptions",
        "-Wno-deprecated-declarations", "-ferror-limit=5", "-fvisibility=hidden"]
args += ["-I" + str(source / path) for path in include]
args += ["-D" + value for value in defines]
args += [str(here / "bridge.cpp")]
args += ["-x", "none", "-framework", "Foundation"]
args += [str(binary / f"UnrealEditor-{module}.dylib") for module in ("Core","CoreUObject","DatasmithCore","DatasmithExporter")]
subprocess.run(args + ["-o", sys.argv[2]], check=True)
