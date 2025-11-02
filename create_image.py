
from PIL import Image

# Create a new white image
img = Image.new('RGB', (100, 100), 'white')

# Save the image as a JPEG
img.save('examples/images/test.jpg', 'jpeg')
