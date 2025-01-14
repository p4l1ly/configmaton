import glob
import os
import sys
import shutil
from setuptools import Command, setup, Extension
from setuptools.command.build_ext import build_ext
from Cython.Build import cythonize

# Get absolute path to the root of the project
ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

def get_lib_name():
    if sys.platform == "win32":
        return "configmaton_ffi.dll"
    elif sys.platform == "darwin":
        return "libconfigmaton_ffi.dylib"
    else:
        return "libconfigmaton_ffi.so"

class RustBuildExt(build_ext):
    def run(self):
        self.run_command("rust_build")

        # Create temporary build directory
        build_temp = os.path.join(self.build_temp, "configmaton")
        os.makedirs(build_temp, exist_ok=True)

        rust_lib = os.path.join(ROOT_DIR, "target", "release", get_lib_name())
        
        print(f"Copying from: {rust_lib}")
        print(f"Copying to: {build_temp}")

        if os.path.exists(rust_lib):
            print(f"Library exists at source")
            shutil.copy2(rust_lib, build_temp)
            if os.path.exists(os.path.join(build_temp, get_lib_name())):
                print(f"Library copied successfully to build dir")
            else:
                print(f"Library copy to build dir failed")
        else:
            raise FileNotFoundError(f"Rust library not found: {rust_lib}")

        # Update library dirs to point to our build directory
        for ext in self.extensions:
            ext.library_dirs = [
                build_temp,
                os.path.join(ROOT_DIR, "target", "release")
            ]

        super().run()

        # After building, copy the library to the same directory as the extension
        ext_path = self.get_ext_fullpath(self.extensions[0].name)
        ext_dir = os.path.dirname(ext_path)
        final_lib = os.path.join(ext_dir, get_lib_name())
        print(f"Copying library to final location: {final_lib}")
        shutil.copy2(os.path.join(build_temp, get_lib_name()), ext_dir)
        if os.path.exists(final_lib):
            print(f"Library copied successfully to final location")
        else:
            print(f"Library copy to final location failed")

class RustBuild(build_ext):
    def run(self):
        ret = os.system(f'cd {ROOT_DIR} && cargo build --release -p configmaton-ffi')
        if ret != 0:
            sys.exit(ret)

class CleanCommand(Command):
    description = "Clean build artifacts"
    user_options = []

    def initialize_options(self):
        pass

    def finalize_options(self):
        pass

    def run(self):
        files_to_delete = [
            "build",
            "dist",
            "*.egg-info",
        ]
        for pattern in files_to_delete:
            for f in glob.glob(pattern):
                print(f"Removing: {f}")
                shutil.rmtree(f) if os.path.isdir(f) else os.remove(f)

ext_modules = cythonize(
    [
        Extension(
            "configmaton.configmaton",
            [os.path.join("configmaton", "configmaton.pyx")],
            libraries=["configmaton_ffi"],
            library_dirs=[os.path.join(ROOT_DIR, "target", "release")],
            include_dirs=[os.path.join(ROOT_DIR, "target", "include")],
            runtime_library_dirs=["$ORIGIN"] if sys.platform != "win32" else [],
        )
    ],
    build_dir="build/cython",  # This controls where .c files go
    compiler_directives={'language_level': "3"},
)

setup(
    name="configmaton",
    version="0.1.0",
    packages=["configmaton"],
    ext_modules=ext_modules,
    cmdclass={
        "rust_build": RustBuild,
        "build_ext": RustBuildExt,
        "clean": CleanCommand,
    },
    package_data={
        'configmaton': ['*.pxd'],
    },
    zip_safe=False,
)
