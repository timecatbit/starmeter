#!/usr/bin/env python3
"""Write the logo as the program and installer icon: one ICO with a PNG for each size Windows asks for."""
import struct
import sys

from PySide6.QtCore import QBuffer, QIODevice
from PySide6.QtWidgets import QApplication

from panel import LOGO_SIZES, logo_pixmap


def png(size):
    buffer=QBuffer();buffer.open(QIODevice.WriteOnly)
    if not logo_pixmap(size).save(buffer,'PNG'):raise OSError('could not draw the icon')
    return bytes(buffer.data())


def icon_bytes():
    images=[png(size) for size in LOGO_SIZES]
    head=struct.pack('<HHH',0,1,len(images));offset=len(head)+16*len(images);entries=b''
    for size,data in zip(LOGO_SIZES,images):
        # 0 in the width and height bytes means 256.
        entries+=struct.pack('<BBBBHHII',size%256,size%256,0,0,1,32,len(data),offset);offset+=len(data)
    return head+entries+b''.join(images)


def main(path):
    # Drawing a pixmap needs a GUI application.
    application=QApplication.instance() or QApplication(sys.argv[:1])
    data=icon_bytes()
    with open(path,'wb') as handle:handle.write(data)
    application.quit()
    return 0


if __name__=='__main__':
    sys.exit(main(sys.argv[1]))
